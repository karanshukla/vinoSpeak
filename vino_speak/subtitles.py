"""Subtitles for saved speech: a cue per spoken sentence, timed from the
samples synthesized for it, as SubRip (.srt) or WebVTT (.vtt)."""

from __future__ import annotations

import textwrap
from dataclasses import dataclass
from pathlib import Path

SUFFIXES = (".srt", ".vtt")
# Subtitle guidelines' two lines of about 42 characters (pinned by
# `test_subtitles.py::test_a_sentence_of_two_lines_is_one_cue_and_longer_is_split`).
LINE_CHARS = 42
CUE_LINES = 2


@dataclass(frozen=True)
class Cue:
    start: float
    end: float
    text: str


def split(start: float, end: float, text: str) -> list[Cue]:
    """One sentence's span as cues of at most CUE_LINES lines, each given
    time in proportion to its characters."""
    lines = textwrap.wrap(text, LINE_CHARS)
    groups = [" ".join(lines[i : i + CUE_LINES]) for i in range(0, len(lines), CUE_LINES)]
    total = sum(len(group) for group in groups) or 1
    cues: list[Cue] = []
    at = start
    for group in groups:
        until = at + (end - start) * len(group) / total
        cues.append(Cue(at, until, group))
        at = until
    if cues:
        cues[-1] = Cue(cues[-1].start, end, cues[-1].text)
    return cues


def _timestamp(seconds: float, separator: str) -> str:
    millis = round(seconds * 1000)
    hours, millis = divmod(millis, 3_600_000)
    minutes, millis = divmod(millis, 60_000)
    secs, millis = divmod(millis, 1000)
    return f"{hours:02}:{minutes:02}:{secs:02}{separator}{millis:03}"


def _lines(text: str) -> str:
    return "\n".join(textwrap.wrap(text, LINE_CHARS))


def srt(cues: list[Cue]) -> str:
    return "".join(
        f"{n}\n{_timestamp(c.start, ',')} --> {_timestamp(c.end, ',')}\n{_lines(c.text)}\n\n"
        for n, c in enumerate(cues, 1)
    )


def vtt(cues: list[Cue]) -> str:
    body = "".join(
        f"{_timestamp(c.start, '.')} --> {_timestamp(c.end, '.')}\n{_lines(c.text)}\n\n"
        for c in cues
    )
    return f"WEBVTT\n\n{body}"


def check_suffix(path: Path) -> None:
    if path.suffix.lower() not in SUFFIXES:
        raise ValueError(f"Subtitles are saved as .srt or .vtt, not '{path.suffix or path.name}'.")


def write(path: Path, cues: list[Cue]) -> None:
    check_suffix(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(vtt(cues) if path.suffix.lower() == ".vtt" else srt(cues), encoding="utf-8")
