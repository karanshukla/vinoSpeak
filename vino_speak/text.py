"""Split text into the chunks that are synthesized one at a time while the
earlier ones play."""

from __future__ import annotations

import re

SENTENCE_BREAK = re.compile(r"(?<=[.!?…])\s+|\n\s*\n")
MAX_CHUNK_CHARS = 400
# Every call pays a fixed cost, so a chunk is never cut smaller than this
MIN_CHUNK_CHARS = 100


def chunks(text: str, max_chars: int = MAX_CHUNK_CHARS) -> list[str]:
    """The first sentence alone, so speech starts quickly, then chunks that
    at most double in length. Synthesis runs at roughly a third of real time
    (2026-10-17, NPU), so each chunk is ready before the one before it has
    finished playing, and the rest of the text goes in as few calls as that
    allows."""
    out: list[str] = []
    budget = 0
    for piece in SENTENCE_BREAK.split(text):
        sentence = " ".join(piece.split())
        if not sentence:
            continue
        if out and len(out[-1]) + len(sentence) <= budget:
            out[-1] = f"{out[-1]} {sentence}"
            continue
        if out:
            budget = min(max_chars, max(2 * len(out[-1]), MIN_CHUNK_CHARS))
        out.append(sentence)
    return out
