"""A text file as something worth listening to: markdown loses its markup,
code blocks are skipped, a document's lines are its sentences, and a URL is
said as its host."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import urlsplit

MARKDOWN_SUFFIXES = frozenset({".md", ".markdown", ".mdx", ".mkd"})
CODE_SKIPPED = "Code skipped."
# A line ending otherwise gets a full stop, so Kokoro pauses and chunks() can
# split after it (pinned by
# `test_readable.py::test_a_document_line_without_punctuation_ends_a_sentence`).
SENTENCE_END = re.compile(r"[.!?…:;]\Z")

FRONT_MATTER = re.compile(r"\A---\n.*?\n(?:---|\.\.\.)\n", re.S)
HTML_COMMENT = re.compile(r"<!--.*?-->", re.S)
# An unclosed fence runs to the end of the file, as it renders (pinned by
# `test_readable.py::test_an_unclosed_fence_skips_to_the_end`).
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,}).*?(?:^ {0,3}\1[`~]*[ \t]*$|\Z)", re.M | re.S)

HEADING = re.compile(r"^ {0,3}#{1,6}\s+(.*?)(?:\s+#+)?\s*$")
UNDERLINE = re.compile(r"^ {0,3}(?:=+|-+)\s*$")
RULE = re.compile(r"^ {0,3}([-*_])(?:\s*\1){2,}\s*$")
QUOTE = re.compile(r"^\s*(?:>\s?)+")
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?")
TABLE_ROW = re.compile(r"^\s*\|.*\|\s*$")
TABLE_RULE = re.compile(r"^[\s|:-]*-[\s|:-]*$")

# Inline patterns stop at the next opening delimiter, never `.+?`, so unclosed
# markup stays linear (pinned by
# `test_readable.py::test_unclosed_inline_markup_is_linear`). A link target may
# hold one level of parentheses (pinned by
# `test_readable.py::test_nested_emphasis_and_parenthesised_link_targets`).
TARGET = r"\((?:[^()]|\([^()]*\))*\)"
IMAGE = re.compile(r"!\[[^\[\]]*\]" + TARGET)
LINK = re.compile(r"\[([^\[\]]*)\]" + TARGET)
REFERENCE_LINK = re.compile(r"\[([^\[\]]+)\]\[[^\[\]]*\]")
REFERENCE = re.compile(r"^ {0,3}\[[^\]]+\]:\s*\S.*$", re.M)
FOOTNOTE = re.compile(r"\[\^[^\[\]]+\]")
AUTOLINK = re.compile(r"<(https?://[^<>\s]+)>")
URL = re.compile(r"https?://[^\s<>()\[\]]*[^\s<>()\[\].,;:!?'\"]")
HTML_TAG = re.compile(r"</?[A-Za-z][^>]*>")
INLINE_CODE = re.compile(r"(`+)(.+?)\1")
STRONG = re.compile(
    r"\*\*(?!\s)((?:[^*]|\*(?!\*))+?)(?<!\s)\*\*|__(?!\s)((?:[^_]|_(?!_))+?)(?<!\s)__"
)
EMPHASIS = re.compile(
    r"(?<![\w*])\*(?!\s)([^*]+?)(?<!\s)\*(?![\w*])|(?<!\w)_(?!\s)([^_]+?)(?<!\s)_(?!\w)"
)
STRIKE = re.compile(r"~~(.+?)~~")
ESCAPE = re.compile(r"\\([\\`*_{}\[\]()#+\-.!|<>~])")
ESCAPED = re.compile("[\ue000-\ue0ff]")

# Only two lowercase halves are rejoined (pinned by
# `test_readable.py::test_a_word_hyphenated_at_a_line_end_is_rejoined`).
HYPHENATED = re.compile(r"([^\W\d_])-[ \t]*\n[ \t]*(\w)")


def _host(match: re.Match[str]) -> str:
    host = urlsplit(match.group(1) if match.re is AUTOLINK else match.group(0)).hostname
    return host.removeprefix("www.") if host else ""


def _inner(match: re.Match[str]) -> str:
    return match.group(1) or match.group(2)


def _dehyphenate(match: re.Match[str]) -> str:
    before, after = match.groups()
    return before + after if before.islower() and after.islower() else f"{before}-{after}"


def _hold_escapes(text: str) -> str:
    """Each escaped character moved into the Private Use Area while the inline
    markup around it is stripped, so `\\*` is never taken for emphasis
    (pinned by `test_readable.py::test_quotes_and_escapes`)."""
    return ESCAPE.sub(lambda m: chr(0xE000 + ord(m.group(1))), text)


def _release_escapes(text: str) -> str:
    return ESCAPED.sub(lambda m: chr(ord(m.group(0)) - 0xE000), text)


def _sentence(line: str) -> str:
    line = line.strip()
    return line if not line or SENTENCE_END.search(line) else f"{line}."


class _SpokenLines:
    """Markdown read a line at a time, each block's markup taken off and each
    heading, list item and table row ended as a sentence."""

    def __init__(self) -> None:
        self.lines: list[str] = []
        self._in_list_item = False

    def add(self, line: str) -> None:
        if self._underlines_a_heading(line):
            self.lines[-1] = _sentence(self.lines[-1])
            self.lines.append("")
        elif RULE.match(line):
            self._end_list_item()
            self.lines.append("")
        elif heading := HEADING.match(line):
            self._end_list_item()
            self.lines.extend(["", _sentence(heading.group(1)), ""])
        elif TABLE_ROW.match(line):
            self._end_list_item()
            if not TABLE_RULE.match(line):
                self.lines.append(_table_row(line))
        elif LIST_ITEM.match(line):
            self._end_list_item()
            self.lines.append(LIST_ITEM.sub("", line, count=1))
            self._in_list_item = True
        else:
            if not line.strip():
                self._end_list_item()
            self.lines.append(QUOTE.sub("", line, count=1))

    def finish(self) -> list[str]:
        self._end_list_item()
        return self.lines

    def _underlines_a_heading(self, line: str) -> bool:
        """A setext underline, with the heading's text on the line above."""
        above = self.lines[-1].strip() if self.lines else ""
        return bool(UNDERLINE.match(line)) and bool(above) and not self._in_list_item

    def _end_list_item(self) -> None:
        """A list item's lines run until a blank line or the next block
        (pinned by `test_readable.py::test_a_wrapped_list_item_ends_once`)."""
        if self._in_list_item:
            self.lines[-1] = _sentence(self.lines[-1])
            self._in_list_item = False


def _table_row(line: str) -> str:
    cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
    return _sentence(", ".join(cell for cell in cells if cell))


def _lines(text: str) -> list[str]:
    spoken = _SpokenLines()
    for line in text.split("\n"):
        spoken.add(line)
    return spoken.finish()


def markdown(text: str) -> str:
    """Markdown as the prose it renders to, block by block, with each heading,
    list item and table row its own sentence."""
    text = FRONT_MATTER.sub("", text.replace("\r\n", "\n"))
    text = HTML_COMMENT.sub("", text)
    text = FENCE.sub(f"\n{CODE_SKIPPED}\n", text)
    text = REFERENCE.sub("", text)
    text = "\n".join(_lines(text))
    text = _hold_escapes(text)
    text = IMAGE.sub("", text)
    text = LINK.sub(r"\1", text)
    text = REFERENCE_LINK.sub(r"\1", text)
    text = FOOTNOTE.sub("", text)
    text = AUTOLINK.sub(_host, text)
    text = plain(text)
    text = HTML_TAG.sub("", text)
    text = INLINE_CODE.sub(r"\2", text)
    text = STRONG.sub(_inner, text)
    text = EMPHASIS.sub(_inner, text)
    text = STRIKE.sub(r"\1", text)
    return _release_escapes(text)


def plain(text: str) -> str:
    """Plain text with each URL shortened to its host."""
    return URL.sub(_host, text)


def document(text: str, wrapped: bool = False) -> str:
    """A document's text, as an extractor gives it, with each line its own
    sentence: a heading, list item, table cell or slide line that ends
    without punctuation gets a full stop, so it isn't run into the next.
    `wrapped` is for a text file on disk, hard-wrapped at a width: there a
    line followed by one starting in lowercase carries on the sentence."""
    text = HYPHENATED.sub(_dehyphenate, text.replace("\r\n", "\n"))
    lines = [line.strip() for line in plain(text).split("\n")]
    return "\n".join(
        line if wrapped and after[:1].islower() else _sentence(line)
        for line, after in zip(lines, [*lines[1:], ""], strict=True)
    )


KINDS = ("plain", "markdown", "document")


def read(path: Path, kind: str | None = None) -> str:
    """The file's text for speaking, read as `kind`. Without one: markdown
    by its suffix, any other file as a hard-wrapped document, and `-` (stdin)
    as plain text. vinoSearch's extracted text comes in on stdin as
    `document`. A file that isn't UTF-8 text is refused."""
    if str(path) == "-":
        text = sys.stdin.read()
    else:
        try:
            text = path.read_bytes().decode("utf-8-sig")
        except UnicodeDecodeError:
            raise ValueError(f"{path} is not a UTF-8 text file.") from None
    if kind is None and str(path) != "-":
        kind = "markdown" if path.suffix.lower() in MARKDOWN_SUFFIXES else "wrapped"
    if kind == "markdown":
        return markdown(text)
    if kind in ("document", "wrapped"):
        return document(text, wrapped=kind == "wrapped")
    return plain(text)
