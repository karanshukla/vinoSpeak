"""Text to speech with Kokoro-82M on an OpenVINO device."""

from __future__ import annotations

import os
import re
import threading
import warnings
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from . import ovfetch
from .model import compile_cache_dir, default_model_dir, ensure_model
from .playback import Speech, require_player
from .text import chunks

if TYPE_CHECKING:
    import numpy as np
    from numpy.typing import NDArray

LANGUAGES = ("en-us", "en-gb", "es", "fr-fr", "hi", "it", "pt-br")
DEFAULT_VOICE = "af_heart"
DEFAULT_DEVICE = "NPU"
# Kokoro's output rate, so a player can start before the first chunk exists.
SAMPLE_RATE = 24000
VOICE_PATTERN = re.compile(r"[a-z]{2}_[a-z0-9_]+\Z")
# OpenVINO 2026.4 pins Kokoro's vocoder to the CPU for an NPU compiler bug
# that driver 1.32.0 on Wildcat Lake doesn't have (2026-10-17). Other drivers
# may, so Synthesizer falls back to the pin if the override fails. Remove
# with #7.
NPU_PROPERTIES = {"NPUW_SUBMODEL_DEVICE": ""}


@dataclass(frozen=True)
class Audio:
    samples: NDArray[np.float32]
    sample_rate: int


def validate_inputs(text: str, voice: str, speed: float, language: str = "en-us") -> None:
    if not text.strip():
        raise ValueError("Text must not be empty.")
    if not VOICE_PATTERN.fullmatch(voice):
        raise ValueError("Voice must be a Kokoro voice identifier, such as 'af_heart'.")
    if not 0.5 <= speed <= 2.0:
        raise ValueError("Speed must be between 0.5 and 2.0.")
    if language not in LANGUAGES:
        raise ValueError(f"Unsupported language '{language}'.")


def load_voice_embedding(voice_path: Path, expected_shape: Any, openvino: Any) -> Any:
    import numpy as np

    values = np.fromfile(voice_path, dtype=np.float32)
    shape = tuple(int(dimension) for dimension in expected_shape)
    expected_size = int(np.prod(shape))
    if values.size != expected_size:
        raise ValueError(
            f"Voice embedding has {values.size} values, but the model expects "
            f"{expected_size} for shape {shape}."
        )
    return openvino.Tensor(values.reshape(shape))


def get_audio_data(speech: Any, openvino: Any) -> NDArray[np.float32]:
    import numpy as np

    try:
        audio = np.asarray(speech.data, dtype=np.float32).reshape(-1)
    except RuntimeError:
        host_tensor = openvino.Tensor(speech.element_type, speech.shape)
        speech.copy_to(host_tensor)
        audio = np.asarray(host_tensor.data, dtype=np.float32).reshape(-1)

    if audio.size == 0 or not np.isfinite(audio).all():
        raise RuntimeError("The generated waveform is empty or contains non-finite values.")
    return audio


class Synthesizer:
    """A loaded Kokoro pipeline. Compiling for the NPU takes seconds, so
    create one and reuse it. Calls are serialized: the pipeline is not
    thread-safe."""

    def __init__(self, device: str = DEFAULT_DEVICE, model_dir: Path | None = None):
        model_dir = ensure_model(model_dir or default_model_dir(), trusted=model_dir is not None)

        try:
            import espeakng_loader
            import openvino as ov
            import openvino_genai as ov_genai
        except ImportError as error:
            raise RuntimeError("Install vinoSpeak's dependencies with 'uv sync'.") from error

        os.environ.setdefault("MISAKI_ESPEAK_LIBRARY", espeakng_loader.get_library_path())
        os.environ.setdefault("ESPEAK_DATA_PATH", espeakng_loader.get_data_path())

        self.device = device
        self.model_dir = model_dir
        self._ov = ov
        self._genai = ov_genai
        # The compiler or driver from `ovfetch npu install`, before the NPU is
        # opened. Only the NPU needs it; elsewhere a bad prefix is not fatal.
        try:
            ovfetch.preload()
        except ovfetch.OvfetchError:
            if device.startswith("NPU"):
                raise
        # Until a generate() has succeeded with the override, a failure (an
        # error, or non-finite samples) may be the compiler bug it exposes,
        # so it is worth one retry with OpenVINO's pin.
        self._unproven_override = False
        self._pipeline = self._load()
        self._voices: dict[str, Any] = {}
        self._lock = threading.Lock()

    def _pipeline_for(self, **properties: str) -> Any:
        # Without a cache every process compiles again: 6.5 s on CPU in the
        # VM, 1.35 s from the cache (2026-10-29).
        cache = str(compile_cache_dir())
        return self._genai.Text2SpeechPipeline(
            self.model_dir, self.device, CACHE_DIR=cache, **properties
        )

    def _load(self) -> Any:
        if self.device.startswith("NPU"):
            try:
                pipeline = self._pipeline_for(**NPU_PROPERTIES)
            except RuntimeError as error:
                self._warn_pinned(error)
            else:
                self._unproven_override = True
                return pipeline
        return self._pipeline_for()

    def _warn_pinned(self, error: Exception) -> None:
        warnings.warn(
            f"The NPU couldn't run Kokoro's vocoder ({error}); falling back to "
            "OpenVINO's default, which runs it on the CPU. Speech works but "
            "saves no battery (vinoSpeak#12).",
            RuntimeWarning,
            stacklevel=3,
        )

    def _generate(self, text: str, voice: str, language: str, speed: float) -> Audio:
        try:
            result = self._pipeline.generate(
                text, self._voice(voice), language=language, speed=float(speed)
            )
            if not result.speeches:
                raise RuntimeError("OpenVINO GenAI did not return any speech.")
            samples = get_audio_data(result.speeches[0], self._ov)
        except RuntimeError as error:
            if not self._unproven_override:
                raise
            self._unproven_override = False
            self._warn_pinned(error)
            self._pipeline = self._pipeline_for()
            return self._generate(text, voice, language, speed)
        self._unproven_override = False
        return Audio(samples, int(result.output_sample_rate))

    def _voice(self, voice: str) -> Any:
        if voice not in self._voices:
            voice_path = self.model_dir / "voices" / f"{voice}.bin"
            if not voice_path.is_file():
                raise ValueError(f"Voice '{voice}' was not found in {voice_path.parent}.")
            self._voices[voice] = load_voice_embedding(
                voice_path, self._pipeline.get_speaker_embedding_shape(), self._ov
            )
        return self._voices[voice]

    def synthesize(
        self,
        text: str,
        voice: str = DEFAULT_VOICE,
        language: str = "en-us",
        speed: float = 1.0,
    ) -> Audio:
        validate_inputs(text, voice, speed, language)
        with self._lock:
            return self._generate(text, voice, language, speed)

    def stream(
        self,
        text: str,
        voice: str = DEFAULT_VOICE,
        language: str = "en-us",
        speed: float = 1.0,
    ) -> Iterator[Audio]:
        """The text a chunk at a time, synthesized as it is consumed. The
        lock is held per chunk, so other callers can interleave."""
        validate_inputs(text, voice, speed, language)
        return (self.synthesize(chunk, voice, language, speed) for chunk in chunks(text))

    def speak(
        self,
        text: str,
        voice: str = DEFAULT_VOICE,
        language: str = "en-us",
        speed: float = 1.0,
        audio_device: str | None = None,
        on_first_audio: Callable[[], None] | None = None,
    ) -> Speech:
        """Start playing the text on the speakers (or `audio_device`) and
        return at once. `on_first_audio` is called once the first samples
        are with the player."""
        return Speech(
            self.stream(text, voice, language, speed),
            audio_device=audio_device,
            sample_rate=SAMPLE_RATE,
            on_first_audio=on_first_audio,
        )


def synthesize(
    text: str,
    output: Path,
    voice: str = DEFAULT_VOICE,
    language: str = "en-us",
    speed: float = 1.0,
    device: str = DEFAULT_DEVICE,
    model_dir: Path | None = None,
) -> Path:
    """One-shot: speak text into a WAV file."""
    # Before the model download and the pipeline compile, not after.
    validate_inputs(text, voice, speed, language)
    try:
        import soundfile as sf
    except ImportError as error:
        raise RuntimeError("Install vinoSpeak's dependencies with 'uv sync'.") from error

    audio = Synthesizer(device, model_dir).synthesize(text, voice, language, speed)
    output.parent.mkdir(parents=True, exist_ok=True)
    sf.write(output, audio.samples, audio.sample_rate)
    return output


def speak(
    text: str,
    voice: str = DEFAULT_VOICE,
    language: str = "en-us",
    speed: float = 1.0,
    device: str = DEFAULT_DEVICE,
    model_dir: Path | None = None,
    audio_device: str | None = None,
) -> None:
    """One-shot: speak text on the speakers (or `audio_device`) and return
    when it has finished."""
    # Before the model download and the pipeline compile, not after.
    validate_inputs(text, voice, speed, language)
    player = require_player()
    synthesizer = Synthesizer(device, model_dir)
    speech = Speech(
        synthesizer.stream(text, voice, language, speed), player, audio_device, SAMPLE_RATE
    )
    try:
        speech.wait()
    finally:
        speech.stop()
