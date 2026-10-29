"""A vinoWhisper session (`.vwsession`) read aloud: its summary or its
transcript. The bundle is read here with zipfile, as vinoSearch reads it, so
vinoWhisper needn't be installed. The format is vinoWhisper's docs/sessions.md."""

from __future__ import annotations

import json
import re
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .readable import markdown

SUFFIX = ".vwsession"
FORMAT = "vinowhisper-session/1"
MIMETYPE = b"application/vnd.vinowhisper.session+zip"
TRANSCRIPT = "transcript.txt"
SUMMARY = "summary.md"
ATTESTATION = "attestation.json"
PARTS = ("summary", "transcript")
# vinoWhisper's own read limits: a bundle can come from anywhere a file
# manager opens one, so the sizes in it aren't trusted.
MAX_PART_BYTES = 64 << 20
MAX_TOTAL_BYTES = 128 << 20
MAX_ENTRIES = 16
MACHINE_WRITTEN = "This summary was machine-written."
MONTHS = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
]

HEADER = re.compile(r"\AvinoWhisper transcript, (\d{4})-(\d{2})-(\d{2}) [^\n]*\n?")
STAMP = re.compile(r"^\[\d{2,}:\d{2}\] ", re.M)
# A summary bullet ends with the stamps it came from: `[01:10]` or `(1:10)`.
TRAILING_STAMPS = re.compile(r"(?:[ \t]*(?:\[\d+:\d{2}\]|\(\d+:\d{2}\)))+[ \t]*$", re.M)
FOOTER = re.compile(r"^\s*[_*]Machine-written summary\..*[_*]\s*$", re.M)


@dataclass(frozen=True)
class Session:
    transcript: str
    summary: str | None
    attestation: dict[str, Any]


def is_session(path: Path) -> bool:
    return path.suffix.lower() == SUFFIX


def load(path: Path) -> Session:
    """The bundle's parts, or ValueError saying why it isn't one this
    version reads."""
    name = path.name
    try:
        with zipfile.ZipFile(path) as archive:
            infos = archive.infolist()
            first = infos[0] if infos else None
            if first is None or first.filename != "mimetype":
                raise ValueError(f"{name} is not a vinoWhisper session: no mimetype first.")
            if first.compress_type != zipfile.ZIP_STORED or archive.read(first) != MIMETYPE:
                raise ValueError(f"{name} is not a vinoWhisper session.")
            if len(infos) > MAX_ENTRIES:
                raise ValueError(f"{name} has too many entries for a session.")
            wanted = [i for i in infos if i.filename in (TRANSCRIPT, SUMMARY, ATTESTATION)]
            if sum(i.file_size for i in wanted) > MAX_TOTAL_BYTES:
                raise ValueError(f"{name} is too large for a session.")
            parts = {i.filename: _read_capped(archive, i, name) for i in wanted}
    except (zipfile.BadZipFile, EOFError) as error:
        raise ValueError(f"{name} is not a vinoWhisper session: {error}") from None
    try:
        attestation = json.loads(parts[ATTESTATION])
        transcript = parts[TRANSCRIPT].decode("utf-8")
        summary = parts[SUMMARY].decode("utf-8") if SUMMARY in parts else None
    except KeyError as error:
        raise ValueError(f"{name} has no {error.args[0]}.") from None
    except (ValueError, UnicodeDecodeError) as error:
        raise ValueError(f"{name}: {error}") from None
    found = attestation.get("format") if isinstance(attestation, dict) else None
    if found != FORMAT:
        raise ValueError(f"{name} is a {found!r} session; this vinoSpeak reads {FORMAT}.")
    return Session(transcript, summary, attestation)


def _read_capped(archive: zipfile.ZipFile, info: zipfile.ZipInfo, name: str) -> bytes:
    # The size in the header can lie; stop at the cap whatever it claims.
    with archive.open(info) as handle:
        data = handle.read(MAX_PART_BYTES + 1)
    if len(data) > MAX_PART_BYTES:
        raise ValueError(f"{name}: {info.filename} is too large.")
    return data


def _sentence(paragraph: str) -> str:
    paragraph = " ".join(paragraph.split())
    return paragraph if not paragraph or paragraph[-1] in ".!?…" else f"{paragraph}."


def transcript(text: str) -> str:
    """The transcript with its header said as the day it was recorded and
    the paragraph stamps dropped; each paragraph ends a sentence."""
    spoken = []
    if header := HEADER.match(text):
        year, month, day = (int(n) for n in header.groups())
        if 1 <= month <= 12:
            spoken.append(f"Recorded on {day} {MONTHS[month - 1]} {year}.")
        text = text[header.end() :]
    paragraphs = re.split(r"\n\s*\n", STAMP.sub("", text))
    spoken.extend(p for p in map(_sentence, paragraphs) if p)
    return "\n\n".join(spoken)


def summary(text: str, machine: bool = False) -> str:
    """The summary as prose, its bullets without their stamps. The footer
    saying it is machine-written is said first instead, so nobody takes
    what follows for the record."""
    machine = machine or bool(FOOTER.search(text))
    spoken = markdown(TRAILING_STAMPS.sub("", FOOTER.sub("", text)))
    spoken = re.sub(r"\n\s*\n\s*", "\n\n", spoken).strip()
    return f"{MACHINE_WRITTEN}\n\n{spoken}" if machine else spoken


def _made_by(attestation: dict[str, Any], part: str) -> object:
    files = attestation.get("files")
    entry = files.get(part) if isinstance(files, dict) else None
    return entry.get("made_by") if isinstance(entry, dict) else None


def read(path: Path, part: str | None = None) -> str:
    """A part of the session for speaking: the summary when there is one,
    else the transcript, unless `part` says which."""
    session = load(path)
    if part is None:
        part = "transcript" if session.summary is None else "summary"
    if part == "transcript":
        return transcript(session.transcript)
    if session.summary is None:
        raise ValueError(f"{path.name} isn't summarised yet; --part transcript reads it.")
    machine = _made_by(session.attestation, SUMMARY) == "ai-generated-summary"
    return summary(session.summary, machine)
