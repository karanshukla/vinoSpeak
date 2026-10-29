"""Command-line Kokoro TTS powered by OpenVINO GenAI."""

from __future__ import annotations

import argparse
from collections.abc import Sequence
from pathlib import Path

from . import readable
from .model import default_model_dir
from .tts import DEFAULT_DEVICE, DEFAULT_VOICE, LANGUAGES, speak, synthesize


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Synthesize speech with Kokoro-82M INT8 on an OpenVINO device."
    )
    parser.add_argument("text", nargs="?", help="Text to synthesize.")
    parser.add_argument(
        "--file",
        type=Path,
        help="Read a text file aloud instead (- for stdin); markdown loses its markup and code blocks.",
    )
    parser.add_argument(
        "--kind",
        choices=readable.KINDS,
        help="Read --file as this instead of by its suffix: document is text extracted from a "
        "PDF, DOCX or slides, one heading, paragraph or table cell a line.",
    )
    parser.add_argument(
        "--print-text",
        action="store_true",
        help="Print the text that would be spoken and exit, without loading the model.",
    )
    parser.add_argument(
        "--output", type=Path, help="Save a WAV here instead of playing it on the speakers."
    )
    parser.add_argument(
        "--audio-device",
        help="Play on this output instead of the default: a PipeWire or PulseAudio sink "
        "name (pactl list short sinks), or an ALSA device.",
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
    if (arguments.text is None) == (arguments.file is None):
        parser.error("give the text to speak or --file, not both")
    if arguments.kind and arguments.file is None:
        parser.error("--kind says how to read --file")
    try:
        text = (
            arguments.text
            if arguments.file is None
            else readable.read(arguments.file, arguments.kind)
        )
    except (OSError, ValueError) as error:
        parser.error(str(error))
    if arguments.audio_device and arguments.output is not None:
        parser.error("--audio-device is where speech plays; it can't go with --output")
    if arguments.print_text:
        print(text)
        return
    options = {
        "voice": arguments.voice,
        "language": arguments.language,
        "speed": arguments.speed,
        "device": arguments.device,
        "model_dir": arguments.model_dir,
    }
    try:
        if arguments.output is None:
            speak(text, audio_device=arguments.audio_device, **options)
            return
        synthesize(text, arguments.output, **options)
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    except KeyboardInterrupt:
        raise SystemExit(130) from None
    print(f"Speech saved to {arguments.output}")


if __name__ == "__main__":
    main()
