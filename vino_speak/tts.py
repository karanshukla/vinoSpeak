"""Text to speech with Kokoro-82M on an OpenVINO device."""

from __future__ import annotations

import os
import threading
import warnings
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

from . import ovfetch, script, ssml, voices
from .lexicon import Lexicon
from .model import compile_cache_dir, default_model_dir, ensure_model
from .playback import Speech, require_player
from .text import speakable_end

if TYPE_CHECKING:
    import numpy as np
    from numpy.typing import NDArray

    from .translate import Translator

LANGUAGES = ("en-us", "en-gb", "es", "fr-fr", "hi", "it", "pt-br")
DEFAULT_LANGUAGE = "en-us"
DEFAULT_VOICE = "af_heart"
DEFAULT_DEVICE = "NPU"
MIN_SPEED, MAX_SPEED = 0.5, 2.0
# Kokoro's output rate, known up front so a player can start before the first
# chunk exists (pinned by
# `test_playback.py::test_with_the_rate_known_the_player_starts_before_the_first_chunk`).
SAMPLE_RATE = 24000
# Undoes OpenVINO 2026.4's CPU pin on Kokoro's vocoder (docs/architecture.md,
# "The NPU vocoder override"). Remove with #7.
NPU_PROPERTIES = {"NPUW_SUBMODEL_DEVICE": ""}
MISSING_DEPENDENCIES = "Install vinoSpeak's dependencies with 'uv sync'."


def is_npu(device: str) -> bool:
    return device.startswith("NPU")


@dataclass(frozen=True)
class Audio:
    samples: NDArray[np.float32]
    sample_rate: int


def check_translatable(text: str, voice: str, speed: float, translate_to: str) -> None:
    from .translate import target_name

    validate_inputs(text, voice, speed)
    target_name(translate_to)
    if ssml.is_ssml(text):
        raise ValueError("SSML can't be translated; give plain text to translate.")


def language_for(voice: str, language: str | None) -> str:
    """The language asked for, or else the one the voice speaks."""
    return language or voices.language_of(voice) or DEFAULT_LANGUAGE


def validate_inputs(text: str, voice: str, speed: float, language: str | None = None) -> None:
    if not text.strip():
        raise ValueError("Text must not be empty.")
    voices.parse(voice)
    if not MIN_SPEED <= speed <= MAX_SPEED:
        raise ValueError(f"Speed must be between {MIN_SPEED} and {MAX_SPEED}.")
    if language is not None and language not in LANGUAGES:
        raise ValueError(f"Unsupported language '{language}'.")


def read_voice(voice_path: Path, expected_shape: Any) -> NDArray[np.float32]:
    import numpy as np

    values = np.fromfile(voice_path, dtype=np.float32)
    shape = tuple(int(dimension) for dimension in expected_shape)
    expected_size = int(np.prod(shape))
    if values.size != expected_size:
        raise ValueError(
            f"Voice embedding has {values.size} values, but the model expects "
            f"{expected_size} for shape {shape}."
        )
    return values.reshape(shape)


def load_voice_embedding(voice_path: Path, expected_shape: Any, openvino: Any) -> Any:
    return openvino.Tensor(read_voice(voice_path, expected_shape))


def blend(parts: list[tuple[NDArray[np.float32], float]]) -> NDArray[np.float32]:
    """The weighted mean of voice embeddings, Kokoro's way of mixing voices
    (pinned by `test_tts.py::test_a_blend_is_the_weighted_mean_of_its_voices`)."""
    import numpy as np

    mixed = np.zeros_like(parts[0][0])
    for embedding, weight in parts:
        mixed += np.float32(weight) * embedding
    return mixed


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


def _preload_npu_compiler(device: str) -> None:
    """Must run before the NPU is opened; a prefix that won't load is fatal
    only when the NPU is the device asked for."""
    try:
        ovfetch.preload()
    except ovfetch.OvfetchError:
        if is_npu(device):
            raise


class Synthesizer:
    """A loaded Kokoro pipeline. Compiling for the NPU takes seconds, so
    create one and reuse it. Calls are serialized: the pipeline is not
    thread-safe."""

    def __init__(
        self,
        device: str = DEFAULT_DEVICE,
        model_dir: Path | None = None,
        lexicon: Lexicon | None = None,
    ):
        model_dir = ensure_model(model_dir or default_model_dir(), trusted=model_dir is not None)

        try:
            import espeakng_loader
            import openvino as ov
            import openvino_genai as ov_genai
        except ImportError as error:
            raise RuntimeError(MISSING_DEPENDENCIES) from error

        os.environ.setdefault("MISAKI_ESPEAK_LIBRARY", espeakng_loader.get_library_path())
        os.environ.setdefault("ESPEAK_DATA_PATH", espeakng_loader.get_data_path())

        self.device = device
        self.model_dir = model_dir
        self.lexicon = lexicon
        self._ov = ov
        self._genai = ov_genai
        _preload_npu_compiler(device)
        # Until one generate() works, a failure gets one retry with OpenVINO's
        # pin (pinned by
        # `test_tts.py::test_a_first_generate_that_fails_on_the_npu_retries_with_the_pin`,
        # `test_tts.py::test_once_the_override_has_worked_errors_are_not_retried`).
        self._unproven_override = False
        self._pipeline = self._load()
        self._voices: dict[str, Any] = {}
        self._lock = threading.Lock()

    def _pipeline_for(self, **properties: str) -> Any:
        cache = str(compile_cache_dir())
        return self._genai.Text2SpeechPipeline(
            self.model_dir, self.device, CACHE_DIR=cache, **properties
        )

    def _load(self) -> Any:
        if is_npu(self.device):
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
        key = voices.canonical(voice)
        if key not in self._voices:
            shape = self._pipeline.get_speaker_embedding_shape()
            parts = []
            for voice_id, weight in voices.parse(voice):
                voice_path = self.model_dir / "voices" / f"{voice_id}.bin"
                if not voice_path.is_file():
                    raise ValueError(f"Voice '{voice_id}' was not found in {voice_path.parent}.")
                parts.append((read_voice(voice_path, shape), weight))
            self._voices[key] = self._ov.Tensor(blend(parts))
        return self._voices[key]

    def synthesize(
        self,
        text: str,
        voice: str = DEFAULT_VOICE,
        language: str | None = None,
        speed: float = 1.0,
    ) -> Audio:
        validate_inputs(text, voice, speed, language)
        language = language_for(voice, language)
        text = speakable_end(text, language)
        if not text:
            import numpy as np

            return Audio(np.zeros(0, np.float32), SAMPLE_RATE)
        with self._lock:
            return self._generate(text, voice, language, speed)

    def plan(
        self,
        text: str,
        voice: str = DEFAULT_VOICE,
        language: str | None = None,
        speed: float = 1.0,
        by_sentence: bool = False,
    ) -> list[script.Step]:
        """The synthesis calls for plain text or SSML, respelled by the lexicon."""
        validate_inputs(text, voice, speed, language)
        return script.plan(text, voice, language, speed, self.lexicon, by_sentence)

    def render(self, step: script.Step) -> Audio:
        if not step.text:
            import numpy as np

            return Audio(np.zeros(round(step.silence * SAMPLE_RATE), np.float32), SAMPLE_RATE)
        return self.synthesize(step.text, step.voice, step.language, step.speed)

    def stream(
        self,
        text: str,
        voice: str = DEFAULT_VOICE,
        language: str | None = None,
        speed: float = 1.0,
        translate_to: str | None = None,
        translator: Translator | None = None,
    ) -> Iterator[Audio]:
        """The text (plain or SSML) a chunk at a time, synthesized as it is
        consumed. The lock is held per chunk, so other callers can interleave.
        With `translate_to`, plain text is translated a piece at a time first,
        and each piece read in that language as soon as it is ready."""
        if translate_to is None:
            return (self.render(step) for step in self.plan(text, voice, language, speed))
        if translator is None:
            raise ValueError("translate_to needs a Translator.")
        check_translatable(text, voice, speed, translate_to)
        return (
            audio
            for piece in translator.stream(text, translate_to)
            for audio in self.stream(piece, voice, translate_to, speed)
        )

    def speak(
        self,
        text: str,
        voice: str = DEFAULT_VOICE,
        language: str | None = None,
        speed: float = 1.0,
        audio_device: str | None = None,
        on_first_audio: Callable[[], None] | None = None,
        volume: float = 1.0,
        translate_to: str | None = None,
        translator: Translator | None = None,
    ) -> Speech:
        """Start playing the text on the speakers (or `audio_device`) and
        return at once. `on_first_audio` is called once the first samples
        are with the player; `volume` (0 to 1) can be changed on the Speech."""
        return Speech(
            self.stream(text, voice, language, speed, translate_to, translator),
            audio_device=audio_device,
            sample_rate=SAMPLE_RATE,
            on_first_audio=on_first_audio,
            volume=volume,
        )


def synthesize(
    text: str,
    output: Path,
    voice: str = DEFAULT_VOICE,
    language: str | None = None,
    speed: float = 1.0,
    device: str = DEFAULT_DEVICE,
    model_dir: Path | None = None,
    subtitles: Path | None = None,
    on_progress: Callable[[int, int], None] | None = None,
    lexicon: Lexicon | None = None,
) -> Path:
    """One-shot: speak text (plain or SSML) into a WAV, FLAC, OGG, Opus or MP3 file (by its
    suffix), and SubRip or WebVTT subtitles if asked for. `on_progress` is
    called with (chunks done, chunks in all)."""
    from .save import check_output, require_soundfile, save
    from .subtitles import check_suffix

    validate_inputs(text, voice, speed, language)
    check_output(output)
    require_soundfile()
    if subtitles is not None:
        check_suffix(subtitles)
    script.plan(text, voice, language, speed, lexicon)
    synthesizer = Synthesizer(device, model_dir, lexicon)
    save(synthesizer, text, output, voice, language, speed, subtitles, on_progress)
    return output


def speak(
    text: str,
    voice: str = DEFAULT_VOICE,
    language: str | None = None,
    speed: float = 1.0,
    device: str = DEFAULT_DEVICE,
    model_dir: Path | None = None,
    audio_device: str | None = None,
    lexicon: Lexicon | None = None,
    translate_to: str | None = None,
    translator: Translator | None = None,
) -> None:
    """One-shot: speak text on the speakers (or `audio_device`) and return
    when it has finished; with `translate_to`, translated first by `translator`."""
    if translate_to is None:
        validate_inputs(text, voice, speed, language)
        script.plan(text, voice, language, speed, lexicon)
    else:
        check_translatable(text, voice, speed, translate_to)
    player = require_player()
    synthesizer = Synthesizer(device, model_dir, lexicon)
    audio = synthesizer.stream(text, voice, language, speed, translate_to, translator)
    speech = Speech(audio, player, audio_device, SAMPLE_RATE)
    try:
        speech.wait()
    finally:
        speech.stop()
