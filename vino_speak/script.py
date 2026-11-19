"""What is said and how, a synthesis call at a time: plain text or SSML,
respelled by the lexicon, cut into chunks (or sentences, for subtitles), and
the silences between them."""

from __future__ import annotations

from dataclasses import dataclass

from . import ssml
from .lexicon import Lexicon
from .text import chunks, sentences


@dataclass(frozen=True)
class Step:
    """One synthesis call, or `silence` seconds of nothing when text is empty."""

    text: str
    voice: str
    language: str | None
    speed: float
    silence: float = 0.0


def clamp_speed(speed: float) -> float:
    """A speed SSML scaled past Kokoro's range, brought back to its edge
    (pinned by `test_script.py::test_a_prosody_rate_past_the_range_is_held_at_its_edge`)."""
    from .tts import MAX_SPEED, MIN_SPEED

    return min(MAX_SPEED, max(MIN_SPEED, speed))


def plan(
    text: str,
    voice: str,
    language: str | None,
    speed: float,
    lexicon: Lexicon | None = None,
    by_sentence: bool = False,
) -> list[Step]:
    parts = ssml.parse(text) if ssml.is_ssml(text) else [ssml.Say(text)]
    cut = sentences if by_sentence else chunks
    steps: list[Step] = []
    for part in parts:
        if isinstance(part, ssml.Pause):
            if part.seconds > 0:
                steps.append(Step("", voice, language, speed, part.seconds))
            continue
        said = lexicon.apply(part.text) if lexicon else part.text
        step_voice = part.voice or voice
        step_language = part.language or (None if part.voice else language)
        rate = clamp_speed(speed * part.rate)
        steps += [Step(piece, step_voice, step_language, rate) for piece in cut(said)]
    if not any(step.text for step in steps):
        raise ValueError("There is no text to speak.")
    return steps


def spoken(steps: list[Step]) -> str:
    """The plan as --print-text shows it: the text, and each pause."""
    return "\n".join(step.text or f"[pause {step.silence:g} s]" for step in steps)
