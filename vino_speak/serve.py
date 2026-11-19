"""`vinospeak --serve`: one loaded Synthesizer kept for the window, so a
reading doesn't start a process and compile the model first. Private to
vinoSpeak's GUI for now; it may change without notice.

Requests, one JSON object a line on stdin:

    {"op": "speak", "id": 1, "text": "...", "voice": "af_heart",
     "language": "en-us" (or null: the voice's own), "speed": 1.0, "audio_device": null, "volume": 1.0}
    {"op": "volume", "volume": 0.5}   (the reading playing, from its next tenth of a second)
    {"op": "stop"}

Events, one a line on stdout:

    {"event": "ready", "load_ms": 4810, "warm_up_ms": 2830}
    {"event": "started", "id": 1}
    {"event": "first-audio", "id": 1, "ms": 812}  (from the request to the first sample played)
    {"event": "done", "id": 1, "stopped": false}
    {"event": "error", "id": 1, "message": "..."}  (id null for a request that didn't parse)

A speak or a stop ends the reading before it; a volume leaves it playing.
The worker exits when stdin closes."""

from __future__ import annotations

import json
import sys
import threading
import time
from collections.abc import Callable, Iterable
from typing import IO, Any, Protocol

from .playback import Speech
from .tts import DEFAULT_VOICE

# Synthesized once after loading and thrown away (pinned by
# `test_serve.py::test_main_warms_up_says_ready_then_serves`).
WARM_UP = "Ready."
OPS = ("speak", "volume", "stop")

Emit = Callable[[dict[str, Any]], None]


class Speaker(Protocol):
    def synthesize(self, text: str, voice: str = ...) -> object: ...

    def speak(
        self,
        text: str,
        voice: str = ...,
        language: str | None = ...,
        speed: float = ...,
        audio_device: str | None = ...,
        on_first_audio: Callable[[], None] | None = ...,
        volume: float = ...,
    ) -> Speech: ...


def emitter(out: IO[str]) -> Emit:
    lock = threading.Lock()

    def emit(event: dict[str, Any]) -> None:
        with lock:
            out.write(json.dumps(event) + "\n")
            out.flush()

    return emit


def volume_of(request: dict[str, Any]) -> float:
    """The request's volume, 1 when it has none (pinned by
    `test_serve.py::test_a_volume_outside_0_to_1_is_refused`)."""
    volume = request.get("volume", 1.0)
    if isinstance(volume, bool) or not isinstance(volume, int | float) or not 0 <= volume <= 1:
        raise ValueError("Volume must be a number from 0 to 1.")
    return float(volume)


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
        volume = volume_of(request)
        emit({"event": "started", "id": self.id})
        self.speech = synthesizer.speak(
            text,
            voice if isinstance(voice, str) else DEFAULT_VOICE,
            language if isinstance(language, str) else None,
            float(speed),
            audio_device if isinstance(audio_device, str) else None,
            on_first_audio=lambda: emit({"event": "first-audio", "id": self.id, "ms": _ms(asked)}),
            volume=volume,
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


def _parse_request(line: str, emit: Emit) -> dict[str, Any] | None:
    """The request on this line, or None once its error event is sent."""
    request: Any = None
    try:
        request = json.loads(line)
        op = request["op"]
    except (ValueError, TypeError, KeyError):
        emit({"event": "error", "id": None, "message": f"Not a request: {line.strip()[:80]}"})
        return None
    if op not in OPS:
        emit({"event": "error", "id": request.get("id"), "message": f"Unknown op {op!r}."})
        return None
    return request  # type: ignore[no-any-return]


def set_volume(reading: Reading | None, request: dict[str, Any], emit: Emit) -> None:
    try:
        volume = volume_of(request)
    except ValueError as error:
        emit({"event": "error", "id": request.get("id"), "message": str(error)})
        return
    if reading is not None:
        reading.speech.volume = volume


def serve(synthesizer: Speaker, lines: Iterable[str], emit: Emit) -> None:
    """Answer requests until `lines` ends, then stop whatever is playing."""
    reading: Reading | None = None
    for line in lines:
        if not line.strip():
            continue
        request = _parse_request(line, emit)
        if request is None:
            continue
        op = request["op"]
        if op == "volume":
            set_volume(reading, request, emit)
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
