"""Speech saved to a file a chunk at a time, in the format its suffix names,
with subtitles timed from the samples if asked for."""

from __future__ import annotations

import contextlib
from collections.abc import Callable
from pathlib import Path
from typing import Any, Protocol

from . import subtitles
from .script import Step
from .tts import MISSING_DEPENDENCIES, SAMPLE_RATE, Audio

# libsndfile's format and encoding for each suffix (pinned by
# `test_save.py::test_each_suffix_is_saved_in_its_own_format`).
FORMATS = {
    ".wav": ("WAV", "PCM_16"),
    ".flac": ("FLAC", "PCM_16"),
    ".ogg": ("OGG", "VORBIS"),
    ".opus": ("OGG", "OPUS"),
    ".mp3": ("MP3", "MPEG_LAYER_III"),
}
PARTIAL = ".partial"

Progress = Callable[[int, int], None]


class Synthesizes(Protocol):
    def plan(
        self,
        text: str,
        voice: str = ...,
        language: str | None = ...,
        speed: float = ...,
        by_sentence: bool = ...,
    ) -> list[Step]: ...

    def render(self, step: Step) -> Audio: ...


def require_soundfile() -> Any:
    try:
        import soundfile
    except ImportError as error:
        raise RuntimeError(MISSING_DEPENDENCIES) from error
    return soundfile


def check_output(path: Path) -> tuple[str, str]:
    """The format and encoding to save `path` in, refused before any model
    loads when its suffix names none."""
    found = FORMATS.get(path.suffix.lower())
    if found is None:
        names = ", ".join(FORMATS)
        raise ValueError(f"Speech is saved as one of {names}, not '{path.suffix or path.name}'.")
    return found


def save(
    synthesizer: Synthesizes,
    text: str,
    output: Path,
    voice: str,
    language: str | None,
    speed: float,
    subtitles_to: Path | None = None,
    on_progress: Progress | None = None,
) -> float:
    """Synthesize the text into `output` and return its length in seconds.
    The file appears only once it is whole. With subtitles each sentence is
    synthesized alone, so its cue starts when it does."""
    sf = require_soundfile()
    container, encoding = check_output(output)
    if subtitles_to is not None:
        subtitles.check_suffix(subtitles_to)
    steps = synthesizer.plan(text, voice, language, speed, by_sentence=subtitles_to is not None)
    cues: list[subtitles.Cue] = []
    at = 0.0
    output.parent.mkdir(parents=True, exist_ok=True)
    partial = output.with_name(output.name + PARTIAL)
    try:
        with sf.SoundFile(partial, "w", SAMPLE_RATE, 1, encoding, format=container) as out:
            for done, step in enumerate(steps, 1):
                audio = synthesizer.render(step)
                if audio.sample_rate != SAMPLE_RATE:
                    raise RuntimeError(f"Speech came at {audio.sample_rate} Hz, not {SAMPLE_RATE}.")
                out.write(audio.samples)
                length = len(audio.samples) / SAMPLE_RATE
                if step.text:
                    cues += subtitles.split(at, at + length, step.written)
                at += length
                if on_progress is not None:
                    on_progress(done, len(steps))
        partial.replace(output)
    finally:
        with contextlib.suppress(FileNotFoundError):
            partial.unlink()
    if subtitles_to is not None:
        subtitles.write(subtitles_to, cues)
    return at
