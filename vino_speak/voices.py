"""Kokoro's voices: what an id like `af_heart` says about a voice, which ones
vinoSpeak can speak, and blends of several (`af_heart(2)+bm_george(1)`).

Nothing here imports numpy or OpenVINO; tts.py does the mixing."""

from __future__ import annotations

import re
from dataclasses import dataclass

from .model import load_pins

# Kokoro's language letters. Japanese (j) and Mandarin (z) are left out:
# OpenVINO GenAI's Kokoro refuses both (pinned by
# `test_voices.py::test_japanese_and_mandarin_voices_are_not_offered`).
ACCENTS = {
    "a": ("American English", "en-us"),
    "b": ("British English", "en-gb"),
    "e": ("Spanish", "es"),
    "f": ("French", "fr-fr"),
    "h": ("Hindi", "hi"),
    "i": ("Italian", "it"),
    "p": ("Brazilian Portuguese", "pt-br"),
}
GENDERS = {"f": "female", "m": "male"}
VOICE_ID = re.compile(r"[a-z]{2}_[a-z0-9_]+\Z")
# One voice of a blend and its optional weight: `af_heart` or `af_heart(0.7)`.
BLEND_PART = re.compile(r"([a-z]{2}_[a-z0-9_]+)(?:\((\d+(?:\.\d+)?|\.\d+)\))?\Z")
# A blend of more than this is refused (pinned by
# `test_voices.py::test_a_blend_of_eight_is_read_and_nine_is_refused`).
MAX_BLEND = 8
NOT_A_VOICE = (
    "Voice must be a Kokoro voice such as 'af_heart', or a blend such as "
    "'af_heart(2)+bm_george(1)'."
)


@dataclass(frozen=True)
class Voice:
    id: str
    accent: str
    gender: str
    language: str

    @property
    def name(self) -> str:
        return " ".join(word.capitalize() for word in self.id[3:].split("_"))


def describe(voice_id: str) -> Voice | None:
    """The voice an id names, or None for one vinoSpeak can't speak."""
    if not VOICE_ID.fullmatch(voice_id) or voice_id[1] not in GENDERS:
        return None
    accent = ACCENTS.get(voice_id[0])
    if accent is None:
        return None
    return Voice(voice_id, accent[0], GENDERS[voice_id[1]], accent[1])


def available() -> list[Voice]:
    """Every voice in the pinned model that vinoSpeak can speak, grouped by
    accent then gender, read from the pins so nothing is downloaded."""
    ids = (
        path.removeprefix("voices/").removesuffix(".bin")
        for path in load_pins().files
        if path.startswith("voices/") and path.endswith(".bin")
    )
    found = [v for v in map(describe, ids) if v is not None]
    order = list(ACCENTS)
    return sorted(found, key=lambda v: (order.index(v.id[0]), v.id[1], v.id))


def parse(spec: str) -> list[tuple[str, float]]:
    """A voice or a blend as (voice id, weight) pairs, the weights summing to 1."""
    parts: list[tuple[str, float]] = []
    for piece in spec.split("+"):
        match = BLEND_PART.fullmatch(piece.strip())
        if match is None:
            raise ValueError(NOT_A_VOICE)
        weight = float(match.group(2) or 1)
        if weight <= 0:
            raise ValueError(f"A blend's weights must be above 0, not {match.group(2)}.")
        parts.append((match.group(1), weight))
    if len(parts) > MAX_BLEND:
        raise ValueError(f"A blend takes at most {MAX_BLEND} voices.")
    total = sum(weight for _, weight in parts)
    return [(voice_id, weight / total) for voice_id, weight in parts]


def canonical(spec: str) -> str:
    """One spelling per blend, so equal blends share one loaded embedding."""
    parts = parse(spec)
    if len(parts) == 1:
        return parts[0][0]
    return "+".join(f"{voice_id}({weight:.4g})" for voice_id, weight in parts)


def language_of(spec: str) -> str | None:
    """The language a voice (or a blend's first voice) is meant to speak, so
    `--voice ef_dora` reads Spanish without `--language es` (pinned by
    `test_voices.py::test_a_voice_brings_its_language`)."""
    accent = ACCENTS.get(parse(spec)[0][0][0])
    return accent[1] if accent else None
