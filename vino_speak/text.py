"""Split text into the chunks that are synthesized one at a time while the
earlier ones play."""

from __future__ import annotations

import re

SENTENCE_BREAK = re.compile(r"(?<=[.!?…])\s+|\n\s*\n")
# Where a sentence too long for one chunk is cut first: after a clause.
CLAUSE_BREAK = re.compile(r"(?<=[,;:—–])\s+")
MAX_CHUNK_CHARS = 400
# Every call pays a fixed cost, so a chunk is never cut smaller than this
MIN_CHUNK_CHARS = 100


def _pieces(sentence: str, max_chars: int) -> list[str]:
    """A sentence longer than max_chars, cut after its clauses, and a clause
    still too long cut between words into pieces of about MIN_CHUNK_CHARS.
    Cutting small keeps the first chunk short however long the sentence is;
    chunks() joins the pieces back up to its budget."""
    if len(sentence) <= max_chars:
        return [sentence]
    out: list[str] = []
    for clause in CLAUSE_BREAK.split(sentence):
        if len(clause) <= max_chars:
            out.append(clause)
            continue
        piece = ""
        for word in clause.split(" "):
            # A single word longer than max_chars is cut too.
            while len(word) > max_chars:
                if piece:
                    out.append(piece)
                    piece = ""
                out.append(word[:max_chars])
                word = word[max_chars:]
            if piece and len(piece) + 1 + len(word) > MIN_CHUNK_CHARS:
                out.append(piece)
                piece = word
            else:
                piece = f"{piece} {word}" if piece else word
        if piece:
            out.append(piece)
    return out


def chunks(text: str, max_chars: int = MAX_CHUNK_CHARS) -> list[str]:
    """The first sentence alone, so speech starts quickly, then chunks that
    at most double in length. Synthesis runs at roughly a third of real time
    (2026-10-17, NPU), so each chunk is ready before the one before it has
    finished playing, and the rest of the text goes in as few calls as that
    allows."""
    out: list[str] = []
    budget = 0
    sentences = (" ".join(piece.split()) for piece in SENTENCE_BREAK.split(text))
    for sentence in (p for s in sentences if s for p in _pieces(s, max_chars)):
        joined = len(out[-1]) + 1 + len(sentence) if out else 0
        if out and len(out[-1]) + len(sentence) <= budget and joined <= max_chars:
            out[-1] = f"{out[-1]} {sentence}"
            continue
        if out:
            budget = min(max_chars, max(2 * len(out[-1]), MIN_CHUNK_CHARS))
        out.append(sentence)
    return out
