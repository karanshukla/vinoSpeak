"""Split text into the chunks that are synthesized one at a time while the
earlier ones play."""

from __future__ import annotations

import re

SENTENCE_BREAK = re.compile(r"(?<=[.!?…])\s+|\n\s*\n")
# A full stop that ends an abbreviation or an initial, not a sentence: a
# chunk cut after `e.g.` falls in pitch mid-sentence.
ABBREVIATION = re.compile(
    r"(?<![^\s(\[\"'“‘])(?:[eE]\.g|[iI]\.e|etc|vs|Dr|Mr|Mrs|Ms|St|No|pp?|[fF]ig|(?:[A-Z]\.)*[A-Z])\.\Z"
)
# Where a sentence too long for one chunk is cut first: after a clause.
CLAUSE_BREAK = re.compile(r"(?<=[,;:—–])\s+")
MAX_CHUNK_CHARS = 400
# Every call pays a fixed cost, so a chunk is never cut smaller than this
MIN_CHUNK_CHARS = 100
# Except the first, which is all the listener waits for: a long first
# sentence is cut at its first clause break past this many characters.
FIRST_CHUNK_CHARS = 60


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


def _sentences(text: str) -> list[str]:
    """The text split after each sentence and at each blank line, but not
    after an abbreviation."""
    out: list[str] = []
    start = 0
    for match in SENTENCE_BREAK.finditer(text):
        paragraph = match.group().count("\n") > 1
        if not paragraph and ABBREVIATION.search(
            text, max(start, match.start() - 16), match.start()
        ):
            continue
        out.append(text[start : match.start()])
        start = match.end()
    out.append(text[start:])
    return out


def _first(sentence: str) -> list[str]:
    """The first sentence cut after its first clause past FIRST_CHUNK_CHARS."""
    cut = CLAUSE_BREAK.search(sentence, FIRST_CHUNK_CHARS)
    if cut is None or cut.end() == len(sentence):
        return [sentence]
    return [sentence[: cut.start()], sentence[cut.end() :]]


def chunks(text: str, max_chars: int = MAX_CHUNK_CHARS) -> list[str]:
    """The first sentence alone, or its first clause if it is long, so
    speech starts quickly, then chunks that at most double in length. Synthesis runs at roughly a third of real time
    (2026-10-17, NPU), so each chunk is ready before the one before it has
    finished playing, and the rest of the text goes in as few calls as that
    allows."""
    out: list[str] = []
    budget = 0
    sentences = (" ".join(piece.split()) for piece in _sentences(text))
    pieces = [p for s in sentences if s for p in _pieces(s, max_chars)]
    pieces[:1] = _first(pieces[0]) if pieces else []
    for sentence in pieces:
        joined = len(out[-1]) + 1 + len(sentence) if out else 0
        if out and len(out[-1]) + len(sentence) <= budget and joined <= max_chars:
            out[-1] = f"{out[-1]} {sentence}"
            continue
        if out:
            budget = min(max_chars, max(2 * len(out[-1]), MIN_CHUNK_CHARS))
        out.append(sentence)
    return out
