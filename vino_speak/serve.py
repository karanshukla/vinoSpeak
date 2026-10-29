"""`vinospeak --serve`: one loaded Synthesizer kept for the window, so a
reading doesn't start a process and compile the model first. Private to
vinoSpeak's GUI for now; it may change without notice.

Requests, one JSON object a line on stdin:

    {"op": "speak", "id": 1, "text": "...", "voice": "af_heart",
     "language": "en-us", "speed": 1.0, "audio_device": null}
    {"op": "stop"}

Events, one a line on stdout:

    {"event": "ready", "load_ms": 4810, "warm_up_ms": 2830}
    {"event": "started", "id": 1}
    {"event": "first-audio", "id": 1, "ms": 812}  (from the request to the first sample played)
    {"event": "done", "id": 1, "stopped": false}
    {"event": "error", "id": 1, "message": "..."}  (id null for a request that didn't parse)

A speak stops the reading before it. The worker exits when stdin closes."""

from __future__ import annotations

import json
import sys
import threading
import time
from collections.abc import Callable, Iterable
from typing import IO, Any, Protocol

from .playback import Speech
from .tts import DEFAULT_VOICE

# Said once after loading and thrown away: the first generate() costs
# ~2.5 s more than a warm one (VM, CPU, 2026-10-29).
WARM_UP = "Ready."

Emit = Callable[[dict[str, Any]], None]


class Speaker(Protocol):
    def synthesize(self, text: str, voice: str = ...) -> object: ...

    def speak(
        self,
        text: str,
        voice: str = ...,
        language: str = ...,
        speed: float = ...,
        audio_device: str | None = ...,
        on_first_audio: Callable[[], None] | None = ...,
    ) -> Speech: ...


def emitter(out: IO[str]) -> Emit:
    lock = threading.Lock()

    def emit(event: dict[str, Any]) -> None:
        with lock:
            out.write(json.dumps(event) + "\n")
            out.flush()

    return emit


def _ms(since: float) -> int:
    return round((time.monotonic() - since) * 1000)


def warm_up(synthesizer: Speaker, emit: Emit, loaded: float) -> None:
    load_ms = _ms(loaded)
    start = time.monotonic()
    synthesizer.synthesize(WARM_UP, DEFAULT_VOICE)
    emit({"event": "ready", "load_ms": load_ms, "warm_up_ms": _ms(start)})


class Reading:
    def __init__(self, synthesizer: Speaker, request: dict[str, Any], emit: Emit) -> None:
        asked = time.monotonic()
        self.id = request.get("id")
        self.emit = emit
        self.stopped = False
        text, voice, language = (request.get(k) for k in ("text", "voice", "language"))
        speed = request.get("speed", 1.0)
        audio_device = request.get("audio_device")
        if not isinstance(text, str) or not isinstance(speed, int | float):
            raise ValueError("A speak request needs text, and a number for speed.")
        emit({"event": "started", "id": self.id})
        self.speech = synthesizer.speak(
            text,
            voice if isinstance(voice, str) else DEFAULT_VOICE,
            language if isinstance(language, str) else "en-us",
            float(speed),
            audio_device if isinstance(audio_device, str) else None,
            on_first_audio=lambda: emit({"event": "first-audio", "id": self.id, "ms": _ms(asked)}),
        )
        self.watcher = threading.Thread(target=self._watch, daemon=True)
        self.watcher.start()

    def _watch(self) -> None:
        try:
            self.speech.wait()
        except Exception as error:  # noqa: BLE001 - reported to the window
            self.emit({"event": "error", "id": self.id, "message": str(error)})
            return
        self.emit({"event": "done", "id": self.id, "stopped": self.stopped})

    def stop(self) -> None:
        self.stopped = True
        self.speech.stop()
        self.watcher.join()


def serve(synthesizer: Speaker, lines: Iterable[str], emit: Emit) -> None:
    """Answer requests until `lines` ends, then stop whatever is playing."""
    reading: Reading | None = None
    for line in lines:
        if not line.strip():
            continue
        request: Any = None
        try:
            request = json.loads(line)
            op = request["op"]
        except (ValueError, TypeError, KeyError):
            emit({"event": "error", "id": None, "message": f"Not a request: {line.strip()[:80]}"})
            continue
        if op not in ("speak", "stop"):
            emit({"event": "error", "id": request.get("id"), "message": f"Unknown op {op!r}."})
            continue
        if reading is not None:
            reading.stop()
            reading = None
        if op == "speak":
            try:
                reading = Reading(synthesizer, request, emit)
            except (OSError, RuntimeError, ValueError) as error:
                emit({"event": "error", "id": request.get("id"), "message": str(error)})
    if reading is not None:
        reading.stop()


def main(synthesizer: Callable[[], Speaker]) -> None:
    """Load, warm up, then serve stdin; a load that fails is an error event
    and exit status 1."""
    emit = emitter(sys.stdout)
    loaded = time.monotonic()
    try:
        speaker = synthesizer()
        warm_up(speaker, emit, loaded)
    except (OSError, RuntimeError, ValueError) as error:
        emit({"event": "error", "id": None, "message": str(error)})
        raise SystemExit(1) from None
    serve(speaker, sys.stdin, emit)
