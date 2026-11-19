"""`vinospeak --http`: OpenAI's speech endpoint served from this machine, so
anything that can talk to `/v1/audio/speech` (Open WebUI, Home Assistant,
editors, scripts) can use vinoSpeak's loaded model.

    POST /v1/audio/speech   {"input": "...", "voice": "af_heart", "speed": 1.0,
                             "response_format": "mp3"}
    GET  /v1/audio/voices   the voices, as Kokoro-FastAPI lists them
    GET  /v1/models         the one model, under OpenAI's names too

Loopback only unless VINOSPEAK_API_KEY is set, and then every request needs
it as a bearer token."""

from __future__ import annotations

import contextlib
import hmac
import io
import ipaddress
import json
import os
import sys
import time
from collections.abc import Callable, Iterator
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Protocol

from . import voices
from .script import Step
from .tts import DEFAULT_VOICE, MAX_SPEED, MIN_SPEED, SAMPLE_RATE, Audio

DEFAULT_HOST = "127.0.0.1"
# Kokoro-FastAPI's port, which clients set up for Kokoro already expect.
DEFAULT_PORT = 8880
API_KEY_VARIABLE = "VINOSPEAK_API_KEY"
# What a request may send (pinned by
# `test_api_server.py::test_input_at_the_cap_is_spoken_and_one_more_character_is_refused`).
MAX_INPUT_CHARS = 100_000
MAX_BODY_BYTES = 1 << 20
MODEL_NAMES = ("kokoro", "tts-1", "tts-1-hd", "gpt-4o-mini-tts")
# OpenAI's voices as Kokoro's of the same name, or the nearest in kind.
OPENAI_VOICES = {
    "alloy": "af_alloy",
    "ash": "am_adam",
    "ballad": "bm_lewis",
    "coral": "af_bella",
    "echo": "am_echo",
    "fable": "bm_fable",
    "nova": "af_nova",
    "onyx": "am_onyx",
    "sage": "af_sarah",
    "shimmer": "af_sky",
    "verse": "am_michael",
}
# libsndfile's format and encoding for each response_format, and its type.
ENCODINGS = {
    "mp3": ("MP3", "MPEG_LAYER_III", "audio/mpeg"),
    "opus": ("OGG", "OPUS", "audio/ogg"),
    "flac": ("FLAC", "PCM_16", "audio/flac"),
    "wav": ("WAV", "PCM_16", "audio/wav"),
}
PCM_TYPE = "audio/pcm"
FORMATS = (*ENCODINGS, "pcm")


class Speaks(Protocol):
    def plan(
        self, text: str, voice: str = ..., language: str | None = ..., speed: float = ...
    ) -> list[Step]: ...

    def render(self, step: Step) -> Audio: ...


class RequestError(ValueError):
    def __init__(self, message: str, status: HTTPStatus = HTTPStatus.BAD_REQUEST) -> None:
        super().__init__(message)
        self.status = status


def is_loopback(host: str) -> bool:
    if host == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def voice_of(name: object) -> str:
    """A Kokoro voice or blend, or one of OpenAI's names for one."""
    if name is None:
        return DEFAULT_VOICE
    if not isinstance(name, str):
        raise RequestError("voice must be a string.")
    voice = OPENAI_VOICES.get(name.lower(), name)
    try:
        voices.parse(voice)
    except ValueError as error:
        raise RequestError(str(error)) from None
    return voice


def speed_of(value: object) -> float:
    """OpenAI's speed, which takes 0.25 to 4, refused outside Kokoro's range
    rather than quietly changed (pinned by
    `test_api_server.py::test_a_speed_outside_kokoros_range_is_refused`)."""
    if value is None:
        return 1.0
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise RequestError("speed must be a number.")
    if not MIN_SPEED <= value <= MAX_SPEED:
        raise RequestError(f"speed must be between {MIN_SPEED} and {MAX_SPEED} for Kokoro.")
    return float(value)


def format_of(value: object) -> str:
    response_format = "mp3" if value is None else value
    if response_format not in FORMATS:
        raise RequestError(f"response_format must be one of {', '.join(FORMATS)}.")
    return str(response_format)


def speech_request(body: dict[str, Any]) -> tuple[str, str, float, str]:
    """The input, voice, speed and format a speech request asks for. Its
    model and instructions are accepted and ignored: there is one model, and
    Kokoro takes no instructions."""
    text = body.get("input")
    if not isinstance(text, str) or not text.strip():
        raise RequestError("input must be the text to speak.")
    if len(text) > MAX_INPUT_CHARS:
        raise RequestError(f"input is over {MAX_INPUT_CHARS} characters.")
    return (
        text,
        voice_of(body.get("voice")),
        speed_of(body.get("speed")),
        format_of(body.get("response_format")),
    )


def pcm(audio: Audio) -> bytes:
    """OpenAI's pcm: 24 kHz mono signed 16-bit little-endian."""
    import numpy as np

    clipped = np.clip(audio.samples, -1.0, 1.0)
    return (clipped * 32767).astype("<i2").tobytes()


def encode(audio: list[Audio], response_format: str) -> bytes:
    from .save import require_soundfile

    sf = require_soundfile()
    container, encoding, _ = ENCODINGS[response_format]
    out = io.BytesIO()
    with sf.SoundFile(out, "w", SAMPLE_RATE, 1, encoding, format=container) as sound:
        for piece in audio:
            sound.write(piece.samples)
    return out.getvalue()


def voice_list() -> dict[str, Any]:
    return {"voices": [voice.id for voice in voices.available()]}


def model_list() -> dict[str, Any]:
    return {
        "object": "list",
        "data": [{"id": name, "object": "model", "owned_by": "vinospeak"} for name in MODEL_NAMES],
    }


class Handler(BaseHTTPRequestHandler):
    """One request. `speaker`, `api_key` and `local_only` are set on the
    subclass make_handler() returns."""

    server_version = "vinospeak"
    speaker: Speaks
    api_key: str | None
    local_only: bool

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002 - the base's name
        sys.stderr.write(f"vinospeak --http: {format % args}\n")

    def _send(self, status: HTTPStatus, body: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _json(self, status: HTTPStatus, record: dict[str, Any]) -> None:
        self._send(status, json.dumps(record).encode(), "application/json")

    def _error(self, error: RequestError) -> None:
        kind = "invalid_request_error" if error.status < 500 else "server_error"
        self._json(error.status, {"error": {"message": str(error), "type": kind}})

    def _check_access(self) -> None:
        """A key when one is set; otherwise a Host naming this machine, so a
        web page can't reach the server through DNS rebinding (pinned by
        `test_api_server.py::test_a_request_for_another_host_is_refused`)."""
        if self.api_key is not None:
            given = self.headers.get("Authorization", "").removeprefix("Bearer ").strip()
            if not hmac.compare_digest(given.encode(), self.api_key.encode()):
                raise RequestError("A valid API key is required.", HTTPStatus.UNAUTHORIZED)
            return
        host = (self.headers.get("Host") or "").rsplit(":", 1)[0].strip("[]")
        if self.local_only and not is_loopback(host):
            raise RequestError(f"Host {host!r} is not this machine.", HTTPStatus.FORBIDDEN)

    def do_GET(self) -> None:  # noqa: N802 - the base's name
        try:
            self._check_access()
            if self.path == "/v1/audio/voices":
                self._json(HTTPStatus.OK, voice_list())
            elif self.path == "/v1/models":
                self._json(HTTPStatus.OK, model_list())
            else:
                raise RequestError(f"No such endpoint: {self.path}", HTTPStatus.NOT_FOUND)
        except RequestError as error:
            self._error(error)

    def do_POST(self) -> None:  # noqa: N802 - the base's name
        try:
            self._check_access()
            if self.path != "/v1/audio/speech":
                raise RequestError(f"No such endpoint: {self.path}", HTTPStatus.NOT_FOUND)
            self._speak(self._body())
        except RequestError as error:
            self._error(error)

    def _body(self) -> dict[str, Any]:
        """The JSON body. Only application/json is taken, so a web page
        can't send one without a CORS preflight, which is never answered."""
        if self.headers.get_content_type() != "application/json":
            raise RequestError(
                "Send the request as application/json.", HTTPStatus.UNSUPPORTED_MEDIA_TYPE
            )
        try:
            length = int(self.headers.get("Content-Length", ""))
        except ValueError:
            raise RequestError("Content-Length is required.", HTTPStatus.LENGTH_REQUIRED) from None
        if not 0 <= length <= MAX_BODY_BYTES:
            raise RequestError("The request is too large.", HTTPStatus.REQUEST_ENTITY_TOO_LARGE)
        try:
            body = json.loads(self.rfile.read(length))
        except ValueError:
            raise RequestError("The body isn't JSON.") from None
        if not isinstance(body, dict):
            raise RequestError("The body must be a JSON object.")
        return body

    def _speak(self, body: dict[str, Any]) -> None:
        text, voice, speed, response_format = speech_request(body)
        try:
            steps = self.speaker.plan(text, voice, None, speed)
        except ValueError as error:
            raise RequestError(str(error)) from None
        if response_format == "pcm":
            self._stream_pcm(self.speaker.render(step) for step in steps)
            return
        try:
            audio = [self.speaker.render(step) for step in steps]
        except (RuntimeError, ValueError) as error:
            raise RequestError(str(error), HTTPStatus.INTERNAL_SERVER_ERROR) from None
        self._send(HTTPStatus.OK, encode(audio, response_format), ENCODINGS[response_format][2])

    def _stream_pcm(self, audio: Iterator[Audio]) -> None:
        """Raw samples sent a chunk at a time as they are synthesized, so a
        client can start playing after the first sentence."""
        try:
            first = next(audio)
        except (RuntimeError, ValueError) as error:
            raise RequestError(str(error), HTTPStatus.INTERNAL_SERVER_ERROR) from None
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", PCM_TYPE)
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        try:
            for piece in _chain(first, audio):
                data = pcm(piece)
                self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
            self.wfile.write(b"0\r\n\r\n")
        except (BrokenPipeError, ConnectionResetError):
            pass  # the client hung up; the rest is never synthesized
        except (RuntimeError, ValueError) as error:
            self.log_message("speech failed mid-stream: %s", error)
            self.close_connection = True


def _chain(first: Audio, rest: Iterator[Audio]) -> Iterator[Audio]:
    yield first
    yield from rest


def make_handler(speaker: Speaks, api_key: str | None, local_only: bool) -> type[Handler]:
    return type(
        "BoundHandler",
        (Handler,),
        {"speaker": speaker, "api_key": api_key, "local_only": local_only},
    )


def address_of(value: str) -> tuple[str, int]:
    """`PORT`, `HOST:PORT` or `[V6]:PORT` as a host and port."""
    host, _, port = value.rpartition(":")
    host = host.strip("[]") or DEFAULT_HOST
    if not port.isdigit() or not 0 < int(port) < 65536:
        raise ValueError(f"--http takes PORT or HOST:PORT, not {value!r}.")
    return host, int(port)


def check_exposure(host: str, api_key: str | None) -> None:
    """Off this machine only with a key (pinned by
    `test_api_server.py::test_a_public_address_needs_a_key`)."""
    if not is_loopback(host) and not api_key:
        raise ValueError(
            f"--http on {host} would let anyone who can reach it use this machine. "
            f"Set {API_KEY_VARIABLE} to a secret every request must send, or listen on 127.0.0.1."
        )


def main(address: str, speaker: Callable[[], Speaks]) -> None:
    host, port = address_of(address)
    api_key = os.environ.get(API_KEY_VARIABLE) or None
    check_exposure(host, api_key)
    started = time.monotonic()
    loaded = speaker()
    handler = make_handler(loaded, api_key, local_only=api_key is None)
    with ThreadingHTTPServer((host, port), handler) as server:
        shown = f"[{host}]" if ":" in host else host
        print(
            f"vinoSpeak is serving OpenAI's speech API at http://{shown}:{server.server_port}/v1 "
            f"(model loaded in {time.monotonic() - started:.1f} s). Ctrl+C stops it.",
            file=sys.stderr,
            flush=True,
        )
        with contextlib.suppress(KeyboardInterrupt):
            server.serve_forever()
