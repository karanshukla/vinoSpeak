"""The part of SSML that Kokoro can honour, read into what to say and how.

Text that starts with `<speak` is SSML. Understood: `<break>`, `<sub>`,
`<prosody rate>`, `<voice name>`, `<lang xml:lang>` (and on `<speak>`),
`<say-as interpret-as="characters|spell-out|digits|telephone">`, and `<p>`
and `<s>` as sentence ends. Any other element is read as its text, the way
Kokoro can't do pitch, volume or emphasis."""

from __future__ import annotations

import re

# Safe here because parse() refuses any DTD first: with no DTD there are no
# entities to expand or fetch, which is all defusedxml would add.
import xml.etree.ElementTree as ET  # nosec B405
from dataclasses import dataclass, replace

from . import voices

XML_LANG = "{http://www.w3.org/XML/1998/namespace}lang"
# SSML's break strengths in seconds, as Azure and Polly roughly time them.
STRENGTHS = {
    "none": 0.0,
    "x-weak": 0.1,
    "weak": 0.25,
    "medium": 0.5,
    "strong": 0.75,
    "x-strong": 1.25,
}
# A longer break is refused, as Polly refuses one over 10 s (pinned by
# `test_ssml.py::test_a_break_of_ten_seconds_is_kept_and_longer_is_refused`).
MAX_BREAK_SECONDS = 10.0
RATES = {"x-slow": 0.5, "slow": 0.75, "medium": 1.0, "fast": 1.25, "x-fast": 1.5, "default": 1.0}
DURATION = re.compile(r"(\d+(?:\.\d+)?)\s*(ms|s)\Z")
PERCENT = re.compile(r"([+-]?\d+(?:\.\d+)?)%\Z")
NUMBER = re.compile(r"\d+(?:\.\d+)?\Z")
SPELLED = frozenset({"characters", "spell-out", "verbatim"})
DIGITS = frozenset({"digits", "telephone"})
# SSML needs no DTD, and refusing one leaves no entities to expand or fetch
# (pinned by `test_ssml.py::test_a_dtd_or_entity_is_refused_before_parsing`).
DTD = re.compile(r"<!\s*(?:DOCTYPE|ENTITY)", re.IGNORECASE)


@dataclass(frozen=True)
class Say:
    text: str
    voice: str | None = None
    language: str | None = None
    rate: float = 1.0


@dataclass(frozen=True)
class Pause:
    seconds: float


Part = Say | Pause


def is_ssml(text: str) -> bool:
    return text.lstrip().startswith("<speak")


def _local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def break_seconds(element: ET.Element) -> float:
    time = element.get("time")
    if time is not None:
        match = DURATION.fullmatch(time.strip())
        if match is None:
            raise ValueError(f"<break time={time!r}>: give it as 500ms or 1.5s.")
        seconds = float(match.group(1)) / (1000 if match.group(2) == "ms" else 1)
    else:
        strength = element.get("strength", "medium")
        if strength not in STRENGTHS:
            raise ValueError(f"<break strength={strength!r}>: one of {', '.join(STRENGTHS)}.")
        seconds = STRENGTHS[strength]
    if seconds > MAX_BREAK_SECONDS:
        raise ValueError(f"A <break> is at most {MAX_BREAK_SECONDS:g} s, not {seconds:g} s.")
    return seconds


def rate_of(value: str) -> float:
    """A prosody rate as a factor of the speed: a name, a percentage
    (`80%`, `+20%`) or a number (`1.2`)."""
    value = value.strip()
    if value in RATES:
        return RATES[value]
    if match := PERCENT.fullmatch(value):
        number = float(match.group(1))
        factor = 1 + number / 100 if value[0] in "+-" else number / 100
    elif NUMBER.fullmatch(value):
        factor = float(value)
    else:
        raise ValueError(f"<prosody rate={value!r}>: a name like slow, a percentage or a number.")
    if factor <= 0:
        raise ValueError(f"<prosody rate={value!r}> must be above zero.")
    return factor


def language_of(value: str) -> str:
    from .tts import LANGUAGES

    language = value.strip().lower().replace("_", "-")
    aliases = {"en": "en-us", "fr": "fr-fr", "pt": "pt-br", "es-es": "es", "hi-in": "hi"}
    language = aliases.get(language, language)
    if language not in LANGUAGES:
        raise ValueError(f"xml:lang={value!r} isn't a language vinoSpeak speaks.")
    return language


def say_as(element: ET.Element) -> str:
    text = "".join(element.itertext())
    interpret = element.get("interpret-as", "")
    if interpret in SPELLED:
        return " ".join(c for c in text if not c.isspace())
    if interpret in DIGITS:
        return " ".join(c for c in text if c.isdigit())
    return text


class _Reader:
    def __init__(self) -> None:
        self.parts: list[Part] = []
        self._text: list[str] = []
        self._style = Say("")

    def _flush(self) -> None:
        text = " ".join("".join(self._text).split())
        self._text = []
        if text:
            self.parts.append(replace(self._style, text=text))

    def _end_sentence(self) -> None:
        if self._text and not re.search(r"[.!?…:;]\s*\Z", "".join(self._text)):
            self._text.append(".")
        self._text.append(" ")

    def _restyle(self, element: ET.Element, tag: str) -> Say:
        style = self._style
        if XML_LANG in element.attrib:
            style = replace(style, language=language_of(element.attrib[XML_LANG]))
        if tag == "voice" and (name := element.get("name")) is not None:
            voices.parse(name)
            style = replace(style, voice=name)
        if tag == "prosody" and (rate := element.get("rate")) is not None:
            style = replace(style, rate=style.rate * rate_of(rate))
        return style

    def read(self, element: ET.Element) -> None:
        tag = _local(element.tag)
        if tag == "break":
            self._flush()
            self.parts.append(Pause(break_seconds(element)))
        elif tag == "sub":
            self._text.append(element.get("alias", "".join(element.itertext())))
        elif tag == "say-as":
            self._text.append(say_as(element))
        else:
            self._read_children(element, tag)
        self._text.append(element.tail or "")

    def _read_children(self, element: ET.Element, tag: str) -> None:
        style = self._restyle(element, tag)
        if style != self._style:
            self._flush()
        outer, self._style = self._style, style
        self._text.append(element.text or "")
        for child in element:
            self.read(child)
        if tag in ("p", "s"):
            self._end_sentence()
        if style != outer:
            self._flush()
        self._style = outer


def parse(text: str) -> list[Part]:
    """SSML as the text to say, each piece with its own voice, language and
    rate, and the pauses between them."""
    if DTD.search(text):
        raise ValueError("SSML can't have a DOCTYPE or entities.")
    try:
        # The DTD check above leaves expat nothing to expand or fetch.
        root = ET.fromstring(text.strip())  # nosec B314
    except ET.ParseError as error:
        raise ValueError(f"The SSML isn't well-formed XML: {error}.") from None
    if _local(root.tag) != "speak":
        raise ValueError("SSML must be one <speak> element.")
    reader = _Reader()
    root.tail = None
    reader.read(root)
    reader._flush()
    return reader.parts
