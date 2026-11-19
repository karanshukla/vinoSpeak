"""Command-line Kokoro TTS powered by OpenVINO GenAI."""

from __future__ import annotations

import argparse
from collections.abc import Sequence
from pathlib import Path

from . import __version__, model, readable, serve, session, voices
from .model import default_model_dir
from .tts import (
    DEFAULT_DEVICE,
    DEFAULT_VOICE,
    LANGUAGES,
    Synthesizer,
    speak,
    synthesize,
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="vinospeak",
        description="Synthesize speech with Kokoro-82M INT8 on an OpenVINO device.",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
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
        "--part",
        choices=session.PARTS,
        help="Which part of a vinoWhisper session (.vwsession) to read "
        "(default: the summary, if it has one).",
    )
    parser.add_argument(
        "--serve",
        action="store_true",
        help="Keep the model loaded and take requests on stdin (vinoSpeak's window uses this; "
        "the protocol is private).",
    )
    parser.add_argument(
        "--model-status",
        action="store_true",
        help=f"Show the downloaded model and OpenVINO's compiled cache in {model.cache_dir()}, "
        "and their size.",
    )
    parser.add_argument(
        "--delete-model",
        action="store_true",
        help="Delete them; the next run downloads the model again. vinoPop's read-aloud "
        "shares them. A --model-dir is never deleted.",
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
    parser.add_argument(
        "--voice",
        default=DEFAULT_VOICE,
        help="Kokoro voice (--list-voices), or a blend of up to "
        f"{voices.MAX_BLEND} with weights: af_heart(2)+bm_george(1).",
    )
    parser.add_argument(
        "--list-voices", action="store_true", help="List the voices and the language of each."
    )
    parser.add_argument(
        "--language",
        choices=LANGUAGES,
        help="Language to read the text as (default: the voice's own, so ef_dora reads Spanish).",
    )
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


def size(byte_count: int) -> str:
    if byte_count >= 1_000_000_000:
        return f"{byte_count / 1e9:.1f} GB"
    if byte_count >= 1_000_000:
        return f"{byte_count / 1e6:.0f} MB"
    return f"{byte_count / 1e3:.0f} kB"


def model_status() -> None:
    found = model.cached()
    if not found:
        print(f"No model downloaded; the first run fetches ~160 MB into {model.cache_dir()}")
        return
    for directory, used in found.items():
        print(f"{size(used):>8}  {directory}")


def delete_model(parser: argparse.ArgumentParser) -> None:
    try:
        freed = model.delete_cached()
    except (OSError, model.ModelBusyError) as error:
        parser.error(str(error))
    if not freed:
        print(f"Nothing to delete in {model.cache_dir()}")
        return
    print(
        f"Deleted the model and its compiled cache ({size(freed)}) from {model.cache_dir()}. "
        "The next run downloads it again."
    )


def list_voices() -> None:
    for voice in voices.available():
        default = "  (default)" if voice.id == DEFAULT_VOICE else ""
        print(f"{voice.id:<14}{voice.language:<7}{voice.gender:<8}{voice.accent}{default}")


def manage_model(parser: argparse.ArgumentParser, arguments: argparse.Namespace) -> None:
    others = (arguments.text, arguments.file, arguments.output, arguments.model_dir)
    if any(o is not None for o in others) or arguments.serve or arguments.print_text:
        parser.error("--model-status and --delete-model take no other options")
    if arguments.delete_model:
        delete_model(parser)
    else:
        model_status()


def serve_requests(parser: argparse.ArgumentParser, arguments: argparse.Namespace) -> None:
    others = (arguments.text, arguments.file, arguments.output, arguments.audio_device)
    if any(o is not None for o in others) or arguments.print_text:
        parser.error("--serve takes its text on stdin; give it only --device and --model-dir")
    serve.main(lambda: Synthesizer(arguments.device, arguments.model_dir))


def text_to_speak(parser: argparse.ArgumentParser, arguments: argparse.Namespace) -> str:
    """The positional text, or --file read as its kind or as a session's part."""
    if (arguments.text is None) == (arguments.file is None):
        parser.error("give the text to speak or --file, not both")
    if arguments.kind and arguments.file is None:
        parser.error("--kind says how to read --file")
    is_session = arguments.file is not None and session.is_session(arguments.file)
    if arguments.part and not is_session:
        parser.error("--part is for a vinoWhisper session: --file x.vwsession")
    if is_session and arguments.kind:
        parser.error("a vinoWhisper session is read as one; --kind doesn't apply")
    try:
        if arguments.file is None:
            text: str = arguments.text
        elif is_session:
            text = session.read(arguments.file, arguments.part)
        else:
            text = readable.read(arguments.file, arguments.kind)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    return text


def play_or_save(parser: argparse.ArgumentParser, arguments: argparse.Namespace, text: str) -> None:
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


def main(argv: Sequence[str] | None = None) -> None:
    parser = build_parser()
    arguments = parser.parse_args(argv)
    if arguments.list_voices:
        list_voices()
        return
    if arguments.model_status or arguments.delete_model:
        manage_model(parser, arguments)
        return
    if arguments.serve:
        serve_requests(parser, arguments)
        return
    text = text_to_speak(parser, arguments)
    if arguments.audio_device and arguments.output is not None:
        parser.error("--audio-device is where speech plays; it can't go with --output")
    if arguments.print_text:
        print(text)
        return
    play_or_save(parser, arguments, text)


if __name__ == "__main__":
    main()
