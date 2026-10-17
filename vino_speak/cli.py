"""Command-line Kokoro TTS powered by OpenVINO GenAI."""

from __future__ import annotations

import argparse
import os
import re
from pathlib import Path
from typing import TYPE_CHECKING, Sequence

if TYPE_CHECKING:
    import numpy as np

MODEL_ID = "OpenVINO/Kokoro-82M-int8-ov"
MODEL_FILES = (
    "openvino_model.xml",
    "openvino_model.bin",
    "config.json",
    "voices/af_heart.bin",
    "data/us_gold.json",
)
LANGUAGES = ("en-us", "en-gb", "es", "fr-fr", "hi", "it", "pt-br")
VOICE_PATTERN = re.compile(r"[a-z]{2}_[a-z0-9_]+\Z")


def validate_inputs(text: str, voice: str, speed: float) -> None:
    if not text.strip():
        raise ValueError("Text must not be empty.")
    if not VOICE_PATTERN.fullmatch(voice):
        raise ValueError("Voice must be a Kokoro voice identifier, such as 'af_heart'.")
    if not 0.5 <= speed <= 2.0:
        raise ValueError("Speed must be between 0.5 and 2.0.")


def model_is_ready(model_dir: Path) -> bool:
    return all((model_dir / filename).is_file() for filename in MODEL_FILES)


def ensure_model(model_dir: Path) -> Path:
    if model_is_ready(model_dir):
        return model_dir

    try:
        from huggingface_hub import snapshot_download
    except ImportError as error:
        raise RuntimeError(
            "huggingface-hub is required to download the Kokoro model."
        ) from error

    model_dir.mkdir(parents=True, exist_ok=True)
    snapshot_download(repo_id=MODEL_ID, local_dir=str(model_dir))
    if not model_is_ready(model_dir):
        raise RuntimeError(
            f"The downloaded model in {model_dir} is incomplete. "
            "Check the model repository and try again."
        )
    return model_dir


def load_voice_embedding(
    voice_path: Path, expected_shape: Sequence[int], openvino: object
) -> object:
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


def get_audio_data(speech: object, openvino: object) -> np.ndarray:
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


def synthesize(
    text: str,
    output: Path,
    voice: str = "af_heart",
    language: str = "en-us",
    speed: float = 1.0,
    device: str = "NPU",
    model_dir: Path | None = None,
) -> Path:
    validate_inputs(text, voice, speed)
    if language not in LANGUAGES:
        raise ValueError(f"Unsupported language '{language}'.")

    if model_dir is None:
        model_dir = Path.home() / ".cache" / "vinospeak" / "kokoro-82m-int8-ov"
    model_dir = ensure_model(model_dir)

    try:
        import espeakng_loader
        import openvino as ov
        import openvino_genai as ov_genai
        import soundfile as sf
    except ImportError as error:
        raise RuntimeError(
            "Install vinoSpeak's dependencies with 'python -m pip install -e .'."
        ) from error

    os.environ.setdefault("MISAKI_ESPEAK_LIBRARY", espeakng_loader.get_library_path())
    os.environ.setdefault("ESPEAK_DATA_PATH", espeakng_loader.get_data_path())

    pipeline = ov_genai.Text2SpeechPipeline(model_dir, device)
    voice_path = model_dir / "voices" / f"{voice}.bin"
    if not voice_path.is_file():
        raise ValueError(f"Voice '{voice}' was not found in {model_dir / 'voices'}.")
    speaker_embedding = load_voice_embedding(
        voice_path, pipeline.get_speaker_embedding_shape(), ov
    )
    result = pipeline.generate(
        text,
        speaker_embedding,
        language=language,
        speed=float(speed),
    )
    if not result.speeches:
        raise RuntimeError("OpenVINO GenAI did not return any speech.")

    audio = get_audio_data(result.speeches[0], ov)
    output.parent.mkdir(parents=True, exist_ok=True)
    sf.write(output, audio, result.output_sample_rate)
    return output


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Synthesize speech with Kokoro-82M INT8 on an OpenVINO device."
    )
    parser.add_argument("text", help="Text to synthesize.")
    parser.add_argument("--output", type=Path, default=Path("speech.wav"))
    parser.add_argument("--voice", default="af_heart", help="Kokoro voice identifier.")
    parser.add_argument("--language", choices=LANGUAGES, default="en-us")
    parser.add_argument("--speed", type=float, default=1.0)
    parser.add_argument(
        "--device",
        default="NPU",
        help="OpenVINO inference device (default: NPU; e.g. CPU or GPU).",
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        help="Model directory (default: ~/.cache/vinospeak/kokoro-82m-int8-ov).",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> None:
    parser = build_parser()
    arguments = parser.parse_args(argv)
    try:
        synthesize(
            arguments.text,
            arguments.output,
            voice=arguments.voice,
            language=arguments.language,
            speed=arguments.speed,
            device=arguments.device,
            model_dir=arguments.model_dir,
        )
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    print(f"Speech saved to {arguments.output}")


if __name__ == "__main__":
    main()
