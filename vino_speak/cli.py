"""Command-line Kokoro TTS powered by OpenVINO GenAI."""

from __future__ import annotations

import argparse
import tempfile
from collections.abc import Sequence
from pathlib import Path

from .model import default_model_dir
from .playback import play
from .tts import DEFAULT_DEVICE, DEFAULT_VOICE, LANGUAGES, synthesize


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Synthesize speech with Kokoro-82M INT8 on an OpenVINO device."
    )
    parser.add_argument("text", help="Text to synthesize.")
    parser.add_argument(
        "--output", type=Path, help="Save a WAV here instead of playing it on the speakers."
    )
    parser.add_argument("--voice", default=DEFAULT_VOICE, help="Kokoro voice identifier.")
    parser.add_argument("--language", choices=LANGUAGES, default="en-us")
    parser.add_argument("--speed", type=float, default=1.0)
    parser.add_argument(
        "--device",
        default=DEFAULT_DEVICE,
        help=f"OpenVINO inference device (default: {DEFAULT_DEVICE}; e.g. CPU or GPU).",
    )
    parser.add_argument(
        "--model-dir",
        type=Path,
        help=f"Model directory, used as given and not checked against the pins "
        f"(default: {default_model_dir()}).",
    )
    return parser


def main(argv: Sequence[str] | None = None) -> None:
    parser = build_parser()
    arguments = parser.parse_args(argv)
    options = {
        "voice": arguments.voice,
        "language": arguments.language,
        "speed": arguments.speed,
        "device": arguments.device,
        "model_dir": arguments.model_dir,
    }
    try:
        if arguments.output is None:
            with tempfile.TemporaryDirectory(prefix="vinospeak-") as scratch:
                play(synthesize(arguments.text, Path(scratch) / "speech.wav", **options))
            return
        synthesize(arguments.text, arguments.output, **options)
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    print(f"Speech saved to {arguments.output}")


if __name__ == "__main__":
    main()
