import io
import json
import threading

import pytest

from vino_speak import cli, serve


class FakeSpeech:
    def __init__(self, on_first_audio, fail=None):
        self.stopped = threading.Event()
        self.on_first_audio = on_first_audio
        self.fail = fail

    def play(self):
        self.on_first_audio()

    def wait(self):
        self.stopped.wait(5)
        if self.fail:
            raise self.fail

    def stop(self):
        self.stopped.set()


class FakeSynthesizer:
    """Plays at once and stops when told, or when the request says "end"."""

    def __init__(self):
        self.warmed = []
        self.requests = []

    def synthesize(self, text, voice="af_heart"):
        self.warmed.append((text, voice))

    def speak(self, text, voice, language, speed, audio_device, on_first_audio=None):
        if not text.strip():
            raise ValueError("Text must not be empty.")
        self.requests.append((text, voice, language, speed, audio_device))
        speech = FakeSpeech(
            on_first_audio, RuntimeError("pw-play exited") if text == "fail" else None
        )
        speech.play()
        if text in ("end", "fail"):
            speech.stop()
        return speech


def run(*requests):
    events = []
    synthesizer = FakeSynthesizer()
    lines = [r if isinstance(r, str) else json.dumps(r) for r in requests]
    serve.serve(synthesizer, lines, events.append)
    return synthesizer, events


def by_id(events, id):
    return [e["event"] for e in events if e.get("id") == id]


def test_a_reading_reports_started_first_audio_and_done():
    synthesizer, events = run(
        {
            "op": "speak",
            "id": 1,
            "text": "end",
            "voice": "bm_george",
            "language": "en-gb",
            "speed": 1.5,
        }
    )
    assert by_id(events, 1) == ["started", "first-audio", "done"]
    assert synthesizer.requests == [("end", "bm_george", "en-gb", 1.5, None)]
    first_audio = next(e for e in events if e["event"] == "first-audio")
    assert isinstance(first_audio["ms"], int)
    assert events[-1] == {"event": "done", "id": 1, "stopped": False}


def test_stop_and_a_new_speak_end_the_reading_before():
    synthesizer, events = run(
        {"op": "speak", "id": 1, "text": "A long read.", "audio_device": "hdmi_sink"},
        {"op": "speak", "id": 2, "text": "Next."},
        {"op": "stop"},
    )
    assert {"event": "done", "id": 1, "stopped": True} in events
    assert {"event": "done", "id": 2, "stopped": True} in events
    assert synthesizer.requests[0] == ("A long read.", "af_heart", "en-us", 1.0, "hdmi_sink")


def test_the_end_of_stdin_stops_what_is_playing():
    _, events = run({"op": "speak", "id": 1, "text": "Unfinished."})
    assert events[-1] == {"event": "done", "id": 1, "stopped": True}


@pytest.mark.parametrize(
    ("request_line", "message"),
    [
        ("not json", "Not a request"),
        ('{"no": "op"}', "Not a request"),
        ('["speak"]', "Not a request"),
        ('{"op": "dance", "id": 3}', "Unknown op 'dance'"),
        ('{"op": "speak", "id": 3}', "needs text"),
        ('{"op": "speak", "id": 3, "text": "x", "speed": "fast"}', "needs text"),
        ('{"op": "speak", "id": 3, "text": " "}', "must not be empty"),
    ],
)
def test_a_bad_request_is_an_error_event_and_the_worker_carries_on(request_line, message):
    _, events = run(request_line, "", {"op": "speak", "id": 4, "text": "end"})
    errors = [e for e in events if e["event"] == "error"]
    assert len(errors) == 1 and message in errors[0]["message"]
    assert by_id(events, 4) == ["started", "first-audio", "done"]


def test_a_reading_that_fails_says_why():
    _, events = run({"op": "speak", "id": 5, "text": "fail"})
    assert events[-1] == {"event": "error", "id": 5, "message": "pw-play exited"}


def test_main_warms_up_says_ready_then_serves(monkeypatch, capsys):
    synthesizer = FakeSynthesizer()
    monkeypatch.setattr(
        "sys.stdin", io.StringIO(json.dumps({"op": "speak", "id": 1, "text": "end"}) + "\n")
    )
    serve.main(lambda: synthesizer)
    events = [json.loads(line) for line in capsys.readouterr().out.splitlines()]
    assert events[0]["event"] == "ready"
    assert {"load_ms", "warm_up_ms"} <= events[0].keys()
    assert synthesizer.warmed == [(serve.WARM_UP, "af_heart")]
    assert [e["event"] for e in events[1:]] == ["started", "first-audio", "done"]


def test_main_reports_a_load_that_fails_and_exits_1(capsys):
    def broken():
        raise RuntimeError("no NPU")

    with pytest.raises(SystemExit) as exit_info:
        serve.main(broken)
    assert exit_info.value.code == 1
    assert json.loads(capsys.readouterr().out) == {
        "event": "error",
        "id": None,
        "message": "no NPU",
    }


def test_cli_serve_loads_the_device_asked_for(monkeypatch):
    loaded = []
    monkeypatch.setattr(cli, "Synthesizer", lambda device, model_dir: loaded.append(device))
    monkeypatch.setattr(cli.serve, "main", lambda make: make())
    cli.main(["--serve", "--device", "CPU"])
    assert loaded == ["CPU"]


@pytest.mark.parametrize(
    "argv", [["Hi"], ["--file", "a.md"], ["--print-text"], ["--output", "a.wav"]]
)
def test_cli_serve_takes_nothing_else(argv):
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["--serve", *argv])
    assert exit_info.value.code == 2
