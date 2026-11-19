"""`vinospeak --serve`: one loaded Synthesizer kept for the window, so a
reading doesn't start a process and compile the model first. Private to
vinoSpeak's GUI for now; it may change without notice.

Requests, one JSON object a line on stdin:

    {"op": "speak", "id": 1, "text": "...", "voice": "af_heart",
     "language": "en-us" (or null: the voice's own), "speed": 1.0, "audio_device": null, "volume": 1.0,
     "translate": null,   (or "es": translated into that language first)
     "from": 0.0}         (or 0.42: read from the sentence that far into the text)
    {"op": "volume", "volume": 0.5}   (the reading playing, from its next tenth of a second)
    {"op": "stop"}

Events, one a line on stdout:

    {"event": "ready", "load_ms": 4810, "warm_up_ms": 2830}
    {"event": "started", "id": 1}
    {"event": "translated", "id": 1, "text": "...", "start": 0.0, "end": 0.12}
        (each piece as it is translated; start and end are where its source
        lies in the whole text, as progress counts it)
    {"event": "first-audio", "id": 1, "ms": 812}  (from the request to the first sample played)
    {"event": "progress", "id": 1, "done": 0.42}  (how far through the whole text playback is, 0 to 1)
    {"event": "done", "id": 1, "stopped": false}
    {"event": "error", "id": 1, "message": "..."}  (id null for a request that didn't parse)

A speak or a stop ends the reading before it; a volume leaves it playing.
The worker exits when stdin closes. The translation model is loaded only for
the first request that asks for a translation."""

from __future__ import annotations

import json
import sys
import threading
import time
from collections.abc import Callable, Iterable, Iterator
from typing import IO, Any, Protocol

from .playback import Speech
from .tts import DEFAULT_VOICE

# Synthesized once after loading and thrown away (pinned by
# `test_serve.py::test_main_warms_up_says_ready_then_serves`).
WARM_UP = "Ready."
OPS = ("speak", "volume", "stop")

Emit = Callable[[dict[str, Any]], None]


PROGRESS_STEP = 0.005


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
        translate_to: str | None = ...,
        translator: Any = ...,
        on_progress: Callable[[float], None] | None = ...,
    ) -> Speech: ...


class Translates(Protocol):
    def stream(self, text: str, language: str) -> Iterator[str]: ...


class Announced:
    """A translator whose every piece is also sent to the window, placed in
    the whole text by `into` when only the rest of it is read."""

    def __init__(
        self,
        translator: Translates,
        emit: Emit,
        reading_id: object,
        into: Callable[[float], float] = lambda x: x,
    ) -> None:
        self._translator = translator
        self._emit = emit
        self._id = reading_id
        self._into = into

    def stream(self, text: str, language: str) -> Iterator[str]:
        """Each piece, sent with its span of the text (pinned by
        `test_serve.py::test_each_translated_piece_is_sent_with_where_it_lies`)."""
        from .translate import spans

        pieces = self._translator.stream(text, language)
        for (start, end), piece in zip(spans(text), pieces, strict=False):
            if piece:
                self._emit(
                    {
                        "event": "translated",
                        "id": self._id,
                        "text": piece,
                        "start": round(self._into(start), 4),
                        "end": round(self._into(end), 4),
                    }
                )
            yield piece


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


def resumed(text: str, request: dict[str, Any]) -> tuple[str, float]:
    """The text from the start of the sentence the request's `from` falls in,
    and where that start lies in the text (pinned by
    `test_serve.py::test_a_reading_from_the_middle_starts_at_its_sentence`)."""
    at = request.get("from", 0.0)
    if isinstance(at, bool) or not isinstance(at, int | float) or not 0 <= at < 1:
        raise ValueError("from must be a number from 0 up to 1.")
    if not at or not text:
        return text, 0.0
    from .ssml import is_ssml
    from .text import sentence_start

    if is_ssml(text):
        raise ValueError("SSML can only be read from the start.")
    start = sentence_start(text, int(at * len(text)))
    return text[start:], start / len(text)


def _ms(since: float) -> int:
    return round((time.monotonic() - since) * 1000)


def warm_up(synthesizer: Speaker, emit: Emit, loaded: float) -> None:
    load_ms = _ms(loaded)
    start = time.monotonic()
    synthesizer.synthesize(WARM_UP, DEFAULT_VOICE)
    emit({"event": "ready", "load_ms": load_ms, "warm_up_ms": _ms(start)})


Translators = Callable[[], Translates]


def _no_translator() -> Translates:
    raise ValueError("This worker can't translate.")


class Reading:
    def __init__(
        self,
        synthesizer: Speaker,
        request: dict[str, Any],
        emit: Emit,
        translators: Translators = _no_translator,
    ) -> None:
        asked = time.monotonic()
        self.id = request.get("id")
        self.emit = emit
        self.stopped = False
        self.shown = 0.0
        text, voice, language = (request.get(k) for k in ("text", "voice", "language"))
        speed = request.get("speed", 1.0)
        audio_device = request.get("audio_device")
        if not isinstance(text, str) or not isinstance(speed, int | float):
            raise ValueError("A speak request needs text, and a number for speed.")
        text, self.offset = resumed(text, request)
        self.shown = self.offset
        volume = volume_of(request)
        target = request.get("translate")
        if target is not None and not isinstance(target, str):
            raise ValueError("translate must be a language, or null.")
        translator = Announced(translators(), emit, self.id, self._whole) if target else None
        emit({"event": "started", "id": self.id})
        self.speech = synthesizer.speak(
            text,
            voice if isinstance(voice, str) else DEFAULT_VOICE,
            language if isinstance(language, str) else None,
            float(speed),
            audio_device if isinstance(audio_device, str) else None,
            on_first_audio=lambda: emit({"event": "first-audio", "id": self.id, "ms": _ms(asked)}),
            volume=volume,
            translate_to=target or None,
            translator=translator,
            on_progress=self._progress,
        )
        self.watcher = threading.Thread(target=self._watch, daemon=True)
        self.watcher.start()

    def _whole(self, done: float) -> float:
        """How far through the whole text `done` of the part read is."""
        return self.offset + (1 - self.offset) * done

    def _progress(self, done: float) -> None:
        """Sent once it has moved a step, so a long reading sends a few hundred
        events, not one per slice (pinned by
        `test_serve.py::test_progress_is_sent_in_steps_up_to_the_end`)."""
        done = self._whole(done)
        if done - self.shown >= PROGRESS_STEP or (done >= 1.0 > self.shown):
            self.shown = done
            self.emit({"event": "progress", "id": self.id, "done": round(done, 3)})

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


def serve(
    synthesizer: Speaker,
    lines: Iterable[str],
    emit: Emit,
    translators: Translators = _no_translator,
) -> None:
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
                reading = Reading(synthesizer, request, emit, translators)
            except (OSError, RuntimeError, ValueError) as error:
                emit({"event": "error", "id": request.get("id"), "message": str(error)})
    if reading is not None:
        reading.stop()


def once(make: Translators) -> Translators:
    """`make`'s translator, made at the first call and kept; a call that
    fails is tried again next time."""
    made: list[Translates] = []

    def get() -> Translates:
        if not made:
            made.append(make())
        return made[0]

    return get


def main(synthesizer: Callable[[], Speaker], translators: Translators = _no_translator) -> None:
    """Load, warm up, then serve stdin; a load that fails is an error event
    and exit status 1. A translator is made only when a request needs one."""
    emit = emitter(sys.stdout)
    loaded = time.monotonic()
    try:
        speaker = synthesizer()
        warm_up(speaker, emit, loaded)
    except (OSError, RuntimeError, ValueError) as error:
        emit({"event": "error", "id": None, "message": str(error)})
        raise SystemExit(1) from None
    serve(speaker, sys.stdin, emit, once(translators))
