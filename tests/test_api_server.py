import http.client
import json
import socket
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from types import ModuleType

import numpy as np
import pytest
from conftest import FakeSoundFile

from vino_speak import api_server, cli, script
from vino_speak.tts import Audio


class FakeSpeaker:
    """Each step as a tenth of a second of a constant, or `fail` raised."""

    def __init__(self, fail=None):
        self.fail = fail
        self.planned = []

    def plan(self, text, voice="af_heart", language=None, speed=1.0):
        self.planned.append((text, voice, language, speed))
        return script.plan(text, voice, language, speed)

    def render(self, step):
        if self.fail is not None:
            raise self.fail
        return Audio(np.full(2400, 0.5, dtype=np.float32), 24000)


@pytest.fixture(autouse=True)
def fake_soundfile(monkeypatch):
    FakeSoundFile.opened.clear()
    soundfile = ModuleType("soundfile")
    soundfile.SoundFile = FakeSoundFile
    monkeypatch.setitem(sys.modules, "soundfile", soundfile)


@pytest.fixture
def serve():
    servers = []

    def start(speaker=None, api_key=None, local_only=True):
        speaker = speaker or FakeSpeaker()
        handler = api_server.make_handler(speaker, api_key, local_only)
        handler.log_message = lambda *args: None
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        servers.append(server)
        return f"http://127.0.0.1:{server.server_port}", speaker

    yield start
    for server in servers:
        server.shutdown()
        server.server_close()


def call(url, body=None, headers=None, raw=None):
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    request = urllib.request.Request(url, data=data, headers=headers or {})
    if data is not None and "Content-Type" not in (headers or {}):
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, response.headers.get_content_type(), response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.headers.get_content_type(), error.read()


def error_of(body):
    return json.loads(body)["error"]["message"]


def test_speech_defaults_to_mp3_in_kokoros_default_voice(serve):
    url, speaker = serve()
    status, kind, body = call(f"{url}/v1/audio/speech", {"model": "tts-1", "input": "Hello."})
    assert (status, kind, body) == (200, "audio/mpeg", b"MP3/MPEG_LAYER_III")
    assert speaker.planned == [("Hello.", "af_heart", None, 1.0)]


@pytest.mark.parametrize(
    ("response_format", "kind", "body"),
    [
        ("opus", "audio/ogg", b"OGG/OPUS"),
        ("flac", "audio/flac", b"FLAC/PCM_16"),
        ("wav", "audio/wav", b"WAV/PCM_16"),
    ],
)
def test_each_response_format_has_its_encoding_and_type(serve, response_format, kind, body):
    url, _ = serve()
    answer = call(f"{url}/v1/audio/speech", {"input": "Hi.", "response_format": response_format})
    assert answer == (200, kind, body)


def test_pcm_is_streamed_as_signed_16_bit_samples(serve):
    url, _ = serve()
    status, kind, body = call(
        f"{url}/v1/audio/speech", {"input": "One. Two.", "response_format": "pcm"}
    )
    assert (status, kind) == (200, "audio/pcm")
    samples = np.frombuffer(body, dtype="<i2")
    assert len(samples) == 2 * 2400
    assert samples[0] == 16383


def test_aac_is_refused_by_name(serve):
    url, _ = serve()
    status, _, body = call(f"{url}/v1/audio/speech", {"input": "Hi.", "response_format": "aac"})
    assert status == 400
    assert "mp3, opus, flac, wav, pcm" in error_of(body)


@pytest.mark.parametrize(
    ("voice", "kokoro"), [("nova", "af_nova"), ("Onyx", "am_onyx"), ("bm_george", "bm_george")]
)
def test_openai_voice_names_are_kokoros_and_kokoros_own_work_too(serve, voice, kokoro):
    url, speaker = serve()
    assert call(f"{url}/v1/audio/speech", {"input": "Hi.", "voice": voice})[0] == 200
    assert speaker.planned[0][1] == kokoro


def test_every_openai_voice_is_a_kokoro_voice_vinospeak_has():
    offered = {voice.id for voice in api_server.voices.available()}
    assert set(api_server.OPENAI_VOICES.values()) <= offered


def test_a_speed_outside_kokoros_range_is_refused(serve):
    url, _ = serve()
    assert call(f"{url}/v1/audio/speech", {"input": "Hi.", "speed": 2.0})[0] == 200
    status, _, body = call(f"{url}/v1/audio/speech", {"input": "Hi.", "speed": 2.01})
    assert status == 400
    assert "between 0.5 and 2.0" in error_of(body)


@pytest.mark.parametrize(
    ("body", "complaint"),
    [
        ({}, "input must be"),
        ({"input": "  "}, "input must be"),
        ({"input": "Hi.", "voice": 3}, "voice must be"),
        ({"input": "Hi.", "voice": "../x"}, "Kokoro voice"),
        ({"input": "Hi.", "speed": "fast"}, "speed must be a number"),
        ({"input": "Hi.", "speed": True}, "speed must be a number"),
        ({"input": "<speak>open"}, "well-formed"),
    ],
)
def test_a_bad_request_says_what_is_wrong(serve, body, complaint):
    url, _ = serve()
    status, kind, answer = call(f"{url}/v1/audio/speech", body)
    assert (status, kind) == (400, "application/json")
    assert complaint in error_of(answer)


def test_input_at_the_cap_is_spoken_and_one_more_character_is_refused(serve, monkeypatch):
    monkeypatch.setattr(api_server, "MAX_INPUT_CHARS", 20)
    url, _ = serve()
    assert call(f"{url}/v1/audio/speech", {"input": "a" * 19 + "."})[0] == 200
    status, _, body = call(f"{url}/v1/audio/speech", {"input": "a" * 20 + "."})
    assert status == 400
    assert "over 20 characters" in error_of(body)


@pytest.mark.parametrize(
    ("raw", "headers", "status"),
    [
        (b'{"input": "x"}', {"Content-Type": "text/plain"}, 415),
        (b"not json", {}, 400),
        (b"[1]", {}, 400),
    ],
)
def test_only_a_json_object_is_taken(serve, raw, headers, status):
    url, _ = serve()
    assert call(f"{url}/v1/audio/speech", raw=raw, headers=headers)[0] == status


def test_a_body_over_the_cap_is_refused_unread(serve, monkeypatch):
    monkeypatch.setattr(api_server, "MAX_BODY_BYTES", 10)
    url, _ = serve()
    assert call(f"{url}/v1/audio/speech", {"input": "Hello there."})[0] == 413


def test_a_failure_while_speaking_is_a_server_error(serve):
    url, _ = serve(FakeSpeaker(fail=RuntimeError("NPU fell over")))
    for response_format in ("mp3", "pcm"):
        status, _, body = call(
            f"{url}/v1/audio/speech", {"input": "Hi.", "response_format": response_format}
        )
        assert status == 500
        assert json.loads(body)["error"] == {"message": "NPU fell over", "type": "server_error"}


def test_voices_and_models_are_listed(serve):
    url, _ = serve()
    status, _, body = call(f"{url}/v1/audio/voices")
    assert status == 200 and "af_heart" in json.loads(body)["voices"]
    status, _, body = call(f"{url}/v1/models")
    assert {model["id"] for model in json.loads(body)["data"]} >= {"kokoro", "tts-1"}


def test_an_unknown_path_is_not_found(serve):
    url, _ = serve()
    assert call(f"{url}/v1/chat/completions", {"input": "x"})[0] == 404
    assert call(f"{url}/")[0] == 404


def test_a_request_for_another_host_is_refused(serve):
    url, _ = serve()
    assert call(f"{url}/v1/models", headers={"Host": "localhost:8880"})[0] == 200
    status, _, body = call(f"{url}/v1/models", headers={"Host": "attacker.example:8880"})
    assert status == 403
    assert "not this machine" in error_of(body)


def test_with_a_key_every_request_must_send_it(serve):
    url, _ = serve(api_key="s3cret", local_only=False)
    assert call(f"{url}/v1/models")[0] == 401
    assert call(f"{url}/v1/models", headers={"Authorization": "Bearer wrong"})[0] == 401
    ok = {"Authorization": "Bearer s3cret", "Host": "speech.example"}
    assert call(f"{url}/v1/models", headers=ok)[0] == 200


def test_a_public_address_needs_a_key():
    api_server.check_exposure("127.0.0.1", None)
    api_server.check_exposure("0.0.0.0", "s3cret")
    with pytest.raises(ValueError, match="VINOSPEAK_API_KEY"):
        api_server.check_exposure("0.0.0.0", None)


@pytest.mark.parametrize(
    ("value", "address"),
    [("8880", ("127.0.0.1", 8880)), ("0.0.0.0:9000", ("0.0.0.0", 9000)), ("[::1]:80", ("::1", 80))],
)
def test_the_address_is_a_port_or_host_and_port(value, address):
    assert api_server.address_of(value) == address


@pytest.mark.parametrize("value", ["http", "host:", "1:99999", "1:0"])
def test_an_address_without_a_port_is_refused(value):
    with pytest.raises(ValueError, match="PORT or HOST:PORT"):
        api_server.address_of(value)


@pytest.mark.parametrize("host", ["localhost", "127.0.0.1", "127.8.8.8", "::1"])
def test_loopback_hosts(host):
    assert api_server.is_loopback(host)


@pytest.mark.parametrize("host", ["0.0.0.0", "192.168.1.2", "example.com", ""])
def test_hosts_that_are_not_loopback(host):
    assert not api_server.is_loopback(host)


def test_cli_http_defaults_to_kokoro_fastapis_port_and_loads_the_lexicon(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
    started = {}

    def main(address, speaker):
        started["address"] = address
        started["speaker"] = speaker()

    monkeypatch.setattr(cli.api_server, "main", main)
    monkeypatch.setattr(
        cli, "Synthesizer", lambda device, model_dir, lexicon: (device, lexicon.words)
    )
    cli.main(["--http", "--device", "CPU"])
    assert started == {"address": "8880", "speaker": ("CPU", {})}


def test_cli_http_reports_a_refused_address(monkeypatch, capsys):
    monkeypatch.delenv("VINOSPEAK_API_KEY", raising=False)
    with pytest.raises(SystemExit):
        cli.main(["--http", "0.0.0.0:8880"])
    assert "VINOSPEAK_API_KEY" in capsys.readouterr().err


@pytest.mark.parametrize("argv", [["Hi"], ["--serve"], ["--output", "a.wav"]])
def test_cli_http_takes_nothing_else(argv):
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["--http", *argv])
    assert exit_info.value.code == 2


class FailsSecond(FakeSpeaker):
    def __init__(self):
        super().__init__()
        self.rendered = 0

    def render(self, step):
        self.rendered += 1
        if self.rendered == 2:
            raise RuntimeError("broke mid-way")
        return super().render(step)


def test_a_failure_mid_stream_cuts_the_stream_short(serve):
    url, _ = serve(FailsSecond())
    with pytest.raises(http.client.IncompleteRead):
        call(f"{url}/v1/audio/speech", {"input": "One. Two.", "response_format": "pcm"})


def test_main_loads_the_model_then_serves_until_ctrl_c(monkeypatch, capsys):
    monkeypatch.delenv("VINOSPEAK_API_KEY", raising=False)
    served = {}

    class Server:
        server_port = 8123

        def __init__(self, address, handler):
            served["address"], served["handler"] = address, handler

        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

        def serve_forever(self):
            raise KeyboardInterrupt

    monkeypatch.setattr(api_server, "ThreadingHTTPServer", Server)
    speaker = FakeSpeaker()
    api_server.main("8123", lambda: speaker)
    assert served["address"] == ("127.0.0.1", 8123)
    assert served["handler"].speaker is speaker and served["handler"].local_only
    assert "http://127.0.0.1:8123/v1" in capsys.readouterr().err


def test_main_with_a_key_checks_the_key_not_the_host(monkeypatch):
    monkeypatch.setenv("VINOSPEAK_API_KEY", "s3cret")
    served = {}

    class Server:
        server_port = 1

        def __init__(self, address, handler):
            served["handler"] = handler

        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

        def serve_forever(self):
            raise KeyboardInterrupt

    monkeypatch.setattr(api_server, "ThreadingHTTPServer", Server)
    api_server.main("[::]:1", FakeSpeaker)
    assert served["handler"].api_key == "s3cret"
    assert not served["handler"].local_only


def test_requests_are_logged_to_stderr(capsys):
    handler = object.__new__(api_server.Handler)
    handler.log_message('"%s" %s', "GET /v1/models", "200")
    assert capsys.readouterr().err == 'vinospeak --http: "GET /v1/models" 200\n'


def test_log_message_escapes_control_characters(capsys):
    handler = object.__new__(api_server.Handler)
    handler.log_message('"%s"', "GET /\x1b[31mpwn\r\n")
    logged = capsys.readouterr().err
    assert "\x1b" not in logged
    assert "\\x1b[31mpwn\\x0d\\x0a" in logged


def test_ipv6_loopback_without_port_and_spoofed_hosts(serve):
    url, _ = serve()
    assert call(f"{url}/v1/models", headers={"Host": "[::1]"})[0] == 200
    assert call(f"{url}/v1/models", headers={"Host": "[::1]:8880"})[0] == 200
    assert call(f"{url}/v1/models", headers={"Host": "evil.com:127.0.0.1"})[0] == 403


def test_a_client_that_sends_nothing_is_dropped(serve, monkeypatch):
    monkeypatch.setattr(api_server.Handler, "timeout", 0.2)
    url, _ = serve()
    with socket.create_connection(tuple(url.removeprefix("http://").split(":"))) as client:
        client.settimeout(5)
        assert client.recv(1) == b""


class LongSpeaker(FakeSpeaker):
    def render(self, step):
        return Audio(np.full(2_000_000, 0.5, dtype=np.float32), 24000)


def test_a_slow_reader_gets_the_whole_stream(serve, monkeypatch):
    monkeypatch.setattr(api_server.Handler, "timeout", 0.2)
    url, _ = serve(LongSpeaker())
    host, port = url.removeprefix("http://").split(":")
    body = json.dumps({"input": "Hello.", "response_format": "pcm"}).encode()
    with socket.socket() as client:
        client.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4096)
        client.connect((host, int(port)))
        client.sendall(
            b"POST /v1/audio/speech HTTP/1.1\r\nHost: 127.0.0.1\r\n"
            b"Content-Type: application/json\r\nConnection: close\r\n"
            + f"Content-Length: {len(body)}\r\n\r\n".encode()
            + body
        )
        time.sleep(1)
        client.settimeout(5)
        received = bytearray()
        while data := client.recv(1 << 16):
            received += data
    assert received.endswith(b"0\r\n\r\n")
    assert len(received) > 4_000_000
