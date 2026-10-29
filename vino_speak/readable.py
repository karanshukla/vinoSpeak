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
# A sentence already ends here; anything else gets a full stop so Kokoro
# pauses and chunks() can split after it.
SENTENCE_END = re.compile(r"[.!?…:;]\Z")

FRONT_MATTER = re.compile(r"\A---\n.*?\n(?:---|\.\.\.)\n", re.S)
HTML_COMMENT = re.compile(r"<!--.*?-->", re.S)
# An unclosed fence runs to the end of the file, as it renders.
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,}).*?(?:^ {0,3}\1[`~]*[ \t]*$|\Z)", re.M | re.S)

HEADING = re.compile(r"^ {0,3}#{1,6}\s+(.*?)(?:\s+#+)?\s*$")
UNDERLINE = re.compile(r"^ {0,3}(?:=+|-+)\s*$")
RULE = re.compile(r"^ {0,3}([-*_])(?:\s*\1){2,}\s*$")
QUOTE = re.compile(r"^\s*(?:>\s?)+")
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?")
TABLE_ROW = re.compile(r"^\s*\|.*\|\s*$")
TABLE_RULE = re.compile(r"^[\s|:-]*-[\s|:-]*$")

# Inline patterns stop at the next opening delimiter, so an unclosed `[` or
# `**` costs a scan to the next one instead of to the end of the line. With
# `.+?`, a line of 5,000 unclosed `**a ` took 3.3 s, growing with the square
# of its length (2026-10-25).
# A link target may hold one level of parentheses, as Wikipedia URLs do.
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
# An escaped character is held in the Private Use Area while the inline
# markup around it is stripped, so `\*` is never taken for emphasis.
ESCAPED = re.compile("[\ue000-\ue0ff]")

# A word broken over a line end: `infor-\nmation` loses the hyphen, but
# `COVID-\n19` and `Jean-\nPaul` keep it, as only lowercase halves can be
# typesetting.
HYPHENATED = re.compile(r"([^\W\d_])-[ \t]*\n[ \t]*(\w)")


def _host(match: re.Match[str]) -> str:
    host = urlsplit(match.group(1) if match.re is AUTOLINK else match.group(0)).hostname
    return host.removeprefix("www.") if host else ""


def _inner(match: re.Match[str]) -> str:
    return match.group(1) or match.group(2)


def _dehyphenate(match: re.Match[str]) -> str:
    before, after = match.groups()
    return before + after if before.islower() and after.islower() else f"{before}-{after}"


def _sentence(line: str) -> str:
    line = line.strip()
    return line if not line or SENTENCE_END.search(line) else f"{line}."


def _lines(text: str) -> list[str]:
    out: list[str] = []
    in_item = False  # a list item's lines run until a blank line or the next block

    def end_item() -> None:
        nonlocal in_item
        if in_item:
            out[-1] = _sentence(out[-1])
            in_item = False

    for line in text.split("\n"):
        if UNDERLINE.match(line) and out and out[-1].strip() and not in_item:
            out[-1] = _sentence(out[-1])  # the line above is a setext heading
            out.append("")
            continue
        if not line.strip() or RULE.match(line) or HEADING.match(line):
            end_item()
        if RULE.match(line):
            out.append("")
        elif heading := HEADING.match(line):
            out.extend(["", _sentence(heading.group(1)), ""])
        elif TABLE_ROW.match(line):
            end_item()
            if not TABLE_RULE.match(line):
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                out.append(_sentence(", ".join(c for c in cells if c)))
        elif LIST_ITEM.match(line):
            end_item()
            out.append(LIST_ITEM.sub("", line, count=1))
            in_item = True
        else:
            out.append(QUOTE.sub("", line, count=1))
    end_item()
    return out


def markdown(text: str) -> str:
    """Markdown as the prose it renders to, block by block, with each heading,
    list item and table row its own sentence."""
    text = FRONT_MATTER.sub("", text.replace("\r\n", "\n"))
    text = HTML_COMMENT.sub("", text)
    text = FENCE.sub(f"\n{CODE_SKIPPED}\n", text)
    text = REFERENCE.sub("", text)
    text = "\n".join(_lines(text))
    text = ESCAPE.sub(lambda m: chr(0xE000 + ord(m.group(1))), text)
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
    return ESCAPED.sub(lambda m: chr(ord(m.group(0)) - 0xE000), text)


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
