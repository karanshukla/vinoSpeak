"""Your own pronunciations: words Kokoro says wrong, respelled as it should
say them, in ~/.config/vinospeak/lexicon.toml:

    [words]
    kubectl = "cube control"
    NGINX = "engine x"
    "Dr. Who" = "Doctor Who"

OpenVINO GenAI's Kokoro ignores inline phonemes, so an entry is a respelling,
not IPA."""

from __future__ import annotations

import os
import re
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

# A lexicon bigger than this is refused (pinned by
# `test_lexicon.py::test_a_lexicon_of_the_cap_is_read_and_one_more_is_refused`).
MAX_ENTRIES = 5000


def default_path() -> Path:
    config = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config")
    return config / "vinospeak" / "lexicon.toml"


def _alternative(word: str) -> str:
    """A word or phrase matched whole, as one numbered group. In lowercase it
    matches any case; with a capital it matches only as written (pinned by
    `test_lexicon.py::test_a_lowercase_entry_matches_any_case_and_a_capitalised_one_only_its_own`)."""
    case = "" if any(c.isupper() for c in word) else "?i"
    return rf"(?<!\w)({f'({case}:' if case else '(?:'}{re.escape(word)}))(?!\w)"


@dataclass
class Lexicon:
    words: dict[str, str] = field(default_factory=dict)

    def __post_init__(self) -> None:
        # Longest first, so "Dr. Who" is tried before "Dr."
        self._respellings = sorted(self.words.items(), key=lambda item: len(item[0]), reverse=True)
        self._pattern = (
            re.compile("|".join(_alternative(w) for w, _ in self._respellings))
            if self._respellings
            else None
        )

    def apply(self, text: str) -> str:
        """The text with each entry respelled, in one pass."""
        if self._pattern is None:
            return text
        return self._pattern.sub(self._respell, text)

    def _respell(self, match: re.Match[str]) -> str:
        return self._respellings[match.lastindex - 1][1]  # type: ignore[operator]


def parse(source: str, origin: str = "lexicon") -> Lexicon:
    try:
        record = tomllib.loads(source)
    except tomllib.TOMLDecodeError as error:
        raise ValueError(f"{origin} isn't valid TOML: {error}") from None
    words = record.get("words", {})
    if not isinstance(words, dict):
        raise ValueError(f'{origin}: [words] must be a table of word = "respelling".')
    if len(words) > MAX_ENTRIES:
        raise ValueError(f"{origin} has {len(words)} words; the most it takes is {MAX_ENTRIES}.")
    for word, respelling in words.items():
        if not word.strip() or not isinstance(respelling, str):
            raise ValueError(f"{origin}: {word!r} must be a word with a respelling in quotes.")
    return Lexicon(dict(words))


def load(path: Path | None = None) -> Lexicon:
    """The lexicon at `path`, or the default one if it exists, else none."""
    if path is None:
        path = default_path()
        if not path.is_file():
            return Lexicon()
    try:
        source = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise ValueError(f"Can't read the lexicon {path}: {error}") from None
    return parse(source, str(path))
