"""Command-line Kokoro TTS powered by OpenVINO GenAI."""

from __future__ import annotations

import argparse
import sys
from collections.abc import Callable, Sequence
from pathlib import Path

from . import (
    __version__,
    api_server,
    lexicon,
    model,
    readable,
    script,
    serve,
    session,
    ssml,
    translate,
    voices,
)
from .model import default_model_dir
from .tts import (
    DEFAULT_DEVICE,
    DEFAULT_VOICE,
    LANGUAGES,
    Synthesizer,
    check_translatable,
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
        "--http",
        metavar="[HOST:]PORT",
        nargs="?",
        const=str(api_server.DEFAULT_PORT),
        help="Keep the model loaded and serve OpenAI's /v1/audio/speech on this port "
        f"(default {api_server.DEFAULT_PORT}, on 127.0.0.1; another host needs "
        f"{api_server.API_KEY_VARIABLE}).",
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
        "--lexicon",
        type=Path,
        help='Respell words from this TOML file ([words] kubectl = "cube control") '
        f"instead of {lexicon.default_path()}, which is used when it exists.",
    )
    parser.add_argument(
        "--print-text",
        action="store_true",
        help="Print the text that would be spoken and exit, without loading the model.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="Save the speech here instead of playing it: .wav, .flac, .ogg, .opus or .mp3.",
    )
    parser.add_argument(
        "--subtitles",
        type=Path,
        help="With --output, also save subtitles timed to it, a sentence a cue: .srt or .vtt.",
    )
    parser.add_argument(
        "--audio-device",
        help="Play on this output instead of the default: a PipeWire or PulseAudio sink "
        "name (pactl list short sinks), or an ALSA device.",
    )
    parser.add_argument(
        "--voice",
        help=f"Kokoro voice (default {DEFAULT_VOICE}, or with --translate one of that "
        "language; --list-voices), or a blend of up to "
        f"{voices.MAX_BLEND} with weights: af_heart(2)+bm_george(1).",
    )
    parser.add_argument(
        "--translate",
        metavar="LANGUAGE",
        choices=translate.TARGETS,
        help=f"Translate the text into this language with {translate.LABEL} first, then read "
        f"it: {', '.join(translate.TARGETS)}. Needs --download-translation-model once.",
    )
    parser.add_argument(
        "--download-translation-model",
        action="store_true",
        help=f"Download the translation model ({translate.LABEL}, about 1.7 GB) "
        f"into {translate.model_dir()}, shared with vinoWhisper.",
    )
    parser.add_argument(
        "--delete-translation-model",
        action="store_true",
        help="Delete it and its compiled copy; a copy vinoWhisper uses is kept.",
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
    if translate.is_downloaded():
        print(f"{size(translate.disk_usage()):>8}  {translate.model_dir()} (translation)")


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


def translation_model(parser: argparse.ArgumentParser, arguments: argparse.Namespace) -> None:
    """--download-translation-model or --delete-translation-model, alone."""
    if arguments.text is not None or arguments.file is not None or arguments.translate:
        parser.error("--download- and --delete-translation-model take no text")
    if arguments.delete_translation_model:
        freed, kept = translate.delete()
        if kept:
            print(
                f"Kept the translation model in {translate.model_dir()}: vinoWhisper uses it too. "
                f"Deleted its compiled copy ({size(freed)})."
            )
        else:
            print(
                f"Deleted the translation model ({size(freed)})." if freed else "Nothing to delete."
            )
        return
    if translate.is_downloaded():
        print(f"The translation model is already in {translate.model_dir()}")
        return
    print(
        f"Downloading {translate.LABEL} ({size(translate.download_bytes())}) "
        f"into {translate.model_dir()}",
        file=sys.stderr,
    )
    try:
        translate.download()
    except (OSError, RuntimeError) as error:
        parser.error(str(error))
    print(f"Downloaded. Read in another language with --translate {', '.join(translate.TARGETS)}")


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


def downloaded_translator(device: str) -> translate.Translator:
    """A translator, once its model is downloaded; it loads at its first use."""
    if not translate.is_downloaded():
        raise ValueError(translate.NOT_DOWNLOADED)
    return translate.Translator(device)


def serve_requests(parser: argparse.ArgumentParser, arguments: argparse.Namespace) -> None:
    others = (arguments.text, arguments.file, arguments.output, arguments.audio_device)
    if any(o is not None for o in others) or arguments.print_text:
        parser.error("--serve takes its text on stdin; give it only --device and --model-dir")
    respellings = load_lexicon(parser, arguments)
    serve.main(
        lambda: Synthesizer(arguments.device, arguments.model_dir, respellings),
        lambda: downloaded_translator(arguments.device),
    )


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


def progress_reporter() -> Callable[[int, int], None] | None:
    """A progress line on stderr while saving, when someone is watching it."""
    if not sys.stderr.isatty():
        return None

    def report(done: int, total: int) -> None:
        end = "\n" if done == total else ""
        print(f"\rSaving: {done} of {total} chunks", end=end, file=sys.stderr, flush=True)

    return report


def serve_http(parser: argparse.ArgumentParser, arguments: argparse.Namespace) -> None:
    others = (arguments.text, arguments.file, arguments.output, arguments.audio_device)
    if any(o is not None for o in others) or arguments.print_text or arguments.serve:
        parser.error(
            "--http takes its text from requests; give it only --device, --model-dir, --lexicon"
        )
    respellings = load_lexicon(parser, arguments)
    try:
        api_server.main(
            arguments.http,
            lambda: Synthesizer(arguments.device, arguments.model_dir, respellings),
        )
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))


def load_lexicon(parser: argparse.ArgumentParser, arguments: argparse.Namespace) -> lexicon.Lexicon:
    try:
        return lexicon.load(arguments.lexicon)
    except ValueError as error:
        parser.error(str(error))


def print_text(
    parser: argparse.ArgumentParser,
    arguments: argparse.Namespace,
    text: str,
    respellings: lexicon.Lexicon,
) -> None:
    """What would be spoken: the text respelled, or SSML's text and pauses."""
    if not ssml.is_ssml(text):
        print(respellings.apply(text))
        return
    try:
        steps = script.plan(text, arguments.voice, arguments.language, arguments.speed, respellings)
    except ValueError as error:
        parser.error(str(error))
    print(script.spoken(steps))


def play_or_save(
    parser: argparse.ArgumentParser,
    arguments: argparse.Namespace,
    text: str,
    respellings: lexicon.Lexicon,
) -> None:
    options = {
        "lexicon": respellings,
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
        synthesize(
            text,
            arguments.output,
            subtitles=arguments.subtitles,
            on_progress=progress_reporter(),
            **options,
        )
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    except KeyboardInterrupt:
        raise SystemExit(130) from None
    print(f"Speech saved to {arguments.output}")
    if arguments.subtitles is not None:
        print(f"Subtitles saved to {arguments.subtitles}")


def translate_and_read(
    parser: argparse.ArgumentParser,
    arguments: argparse.Namespace,
    text: str,
    respellings: lexicon.Lexicon,
) -> None:
    """--translate: the model is checked for, never downloaded, then the text
    is translated and printed, saved or spoken a piece at a time."""
    target = arguments.translate
    if arguments.language is not None:
        parser.error("--translate says which language to read in; drop --language")
    arguments.voice = translate.voice_for(target, arguments.voice)
    try:
        check_translatable(text, arguments.voice, arguments.speed, target)
    except ValueError as error:
        parser.error(str(error))
    if not translate.is_downloaded():
        parser.error(translate.NOT_DOWNLOADED)
    translator = translate.Translator(arguments.device)
    try:
        if arguments.print_text:
            print(translator.translate(text, target))
            return
        if arguments.output is not None:
            arguments.language = target
            text = translator.translate(text, target)
        else:
            speak(
                text,
                voice=arguments.voice,
                speed=arguments.speed,
                device=arguments.device,
                model_dir=arguments.model_dir,
                audio_device=arguments.audio_device,
                lexicon=respellings,
                translate_to=target,
                translator=translator,
            )
            return
    except (OSError, RuntimeError, ValueError) as error:
        parser.error(str(error))
    except KeyboardInterrupt:
        raise SystemExit(130) from None
    play_or_save(parser, arguments, text, respellings)


def main(argv: Sequence[str] | None = None) -> None:
    parser = build_parser()
    arguments = parser.parse_args(argv)
    if arguments.list_voices:
        list_voices()
        return
    if arguments.download_translation_model or arguments.delete_translation_model:
        translation_model(parser, arguments)
        return
    if arguments.model_status or arguments.delete_model:
        manage_model(parser, arguments)
        return
    if arguments.http is not None:
        serve_http(parser, arguments)
        return
    if arguments.serve:
        serve_requests(parser, arguments)
        return
    text = text_to_speak(parser, arguments)
    if arguments.subtitles is not None and arguments.output is None:
        parser.error("--subtitles are timed to a saved file: give --output too")
    if arguments.audio_device and arguments.output is not None:
        parser.error("--audio-device is where speech plays; it can't go with --output")
    respellings = load_lexicon(parser, arguments)
    if arguments.translate:
        translate_and_read(parser, arguments, text, respellings)
        return
    arguments.voice = arguments.voice or DEFAULT_VOICE
    if arguments.print_text:
        print_text(parser, arguments, text, respellings)
        return
    play_or_save(parser, arguments, text, respellings)


if __name__ == "__main__":
    main()
