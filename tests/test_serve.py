import io
import json
import sys
import threading
from types import ModuleType

import numpy as np
import pytest
from conftest import FakeSoundFile

from vino_speak import cli, script, serve
from vino_speak.lexicon import Lexicon
from vino_speak.tts import Audio


class FakeSpeech:
    def __init__(self, on_first_audio, fail=None):
        self.stopped = threading.Event()
        self.on_first_audio = on_first_audio
        self.fail = fail
        self.volume = 1.0

    def play(self):
        self.on_first_audio()

    def wait(self):
        self.stopped.wait(5)
        if self.fail:
            raise self.fail

    def stop(self):
        self.stopped.set()

    def pause(self):
        self.paused = True

    def resume(self):
        self.paused = False


class FakeSynthesizer:
    """Plays at once and stops when told, or when the request says "end"."""

    def __init__(self):
        self.warmed = []
        self.requests = []
        self.speeches = []
        self.rendered = []
        self.lexicon = None
        self.render_gate = None

    def plan(self, text, voice="af_heart", language=None, speed=1.0, by_sentence=False):
        return script.plan(text, voice, language, speed, self.lexicon, by_sentence)

    def render(self, step):
        if self.render_gate is not None:
            self.render_gate.wait(5)
        self.rendered.append((step.text, step.voice, step.language))
        return Audio(np.zeros(2400, dtype=np.float32), 24000)

    def synthesize(self, text, voice="af_heart"):
        self.warmed.append((text, voice))

    def speak(
        self,
        text,
        voice,
        language,
        speed,
        audio_device,
        on_first_audio=None,
        volume=1.0,
        translate_to=None,
        translator=None,
        on_progress=None,
    ):
        if not text.strip():
            raise ValueError("Text must not be empty.")
        self.requests.append((text, voice, language, speed, audio_device))
        self.on_progress = on_progress
        if translate_to is not None:
            self.translated = list(translator.stream(text, translate_to))
        speech = FakeSpeech(
            on_first_audio, RuntimeError("pw-play exited") if text == "fail" else None
        )
        speech.volume = volume
        self.speeches.append(speech)
        speech.play()
        if text in ("end", "fail"):
            speech.stop()
        return speech


def run(*requests, translators=serve._no_translator, lexicons=Lexicon, synthesizer=None):
    events = []
    synthesizer = synthesizer or FakeSynthesizer()
    lines = [r if isinstance(r, str) else json.dumps(r) for r in requests]
    serve.serve(synthesizer, lines, events.append, translators, lexicons)
    return synthesizer, events


@pytest.fixture
def fake_soundfile(monkeypatch):
    FakeSoundFile.opened.clear()
    soundfile = ModuleType("soundfile")
    soundfile.SoundFile = FakeSoundFile
    monkeypatch.setitem(sys.modules, "soundfile", soundfile)


def until(events, kind):
    """Wait for a thread of the worker's to send an event of this kind."""
    import time

    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if any(e["event"] == kind for e in list(events)):
            return
        time.sleep(0.01)
    raise AssertionError(f"no {kind} event in {events}")


def then_wait(events, kind, *requests):
    """The requests as lines, then stdin held open until an event of `kind`,
    as the window holds it open: its closing cancels a save."""
    for request in requests:
        yield json.dumps(request)
    until(events, kind)


def save_request(tmp_path, **extra):
    return {
        "op": "save",
        "id": 9,
        "text": "One sentence. And another one.",
        "voice": "bm_george",
        "language": "en-gb",
        "speed": 1.0,
        "output": str(tmp_path / "notes.mp3"),
        **extra,
    }


def test_a_save_writes_the_file_and_its_subtitles_with_progress(tmp_path, fake_soundfile):
    synthesizer, events = FakeSynthesizer(), []
    request = save_request(tmp_path, subtitles=str(tmp_path / "notes.srt"))
    serve.serve(synthesizer, then_wait(events, "saved", request), events.append)
    assert (tmp_path / "notes.mp3").read_bytes() == b"MP3/MPEG_LAYER_III"
    assert "One sentence." in (tmp_path / "notes.srt").read_text()
    assert synthesizer.rendered[0][1:] == ("bm_george", "en-gb")
    saving = [e["done"] for e in events if e["event"] == "saving"]
    assert saving == sorted(saving) and saving[-1] == 1.0
    assert events[-1] == {"event": "saved", "id": 9, "seconds": 0.2}


def test_a_save_goes_on_beside_the_reading(tmp_path, fake_soundfile):
    synthesizer, events = FakeSynthesizer(), []
    lines = then_wait(
        events, "saved", {"op": "speak", "id": 1, "text": "Hello"}, save_request(tmp_path)
    )
    serve.serve(synthesizer, lines, events.append)
    kinds = [e["event"] for e in events]
    assert kinds.index("saved") < kinds.index("done")
    assert not any(e["event"] == "error" for e in events)


def test_a_cancelled_save_leaves_nothing_and_says_so(tmp_path, fake_soundfile):
    synthesizer, events = FakeSynthesizer(), []
    synthesizer.render_gate = threading.Event()
    saving = serve.Saving(synthesizer, save_request(tmp_path), events.append, serve._no_translator)
    until_rendering = threading.Timer(0.05, synthesizer.render_gate.set)
    saving.cancelled.set()
    until_rendering.start()
    saving.thread.join(5)
    assert events == [{"event": "save-cancelled", "id": 9}]
    assert list(tmp_path.iterdir()) == []


def test_cancel_save_and_a_second_save_cancel_the_one_under_way(tmp_path, fake_soundfile):
    synthesizer, events = FakeSynthesizer(), []
    synthesizer.render_gate = threading.Event()
    threading.Timer(0.2, synthesizer.render_gate.set).start()
    second = save_request(tmp_path, id=10, output=str(tmp_path / "b.wav"))
    lines = then_wait(events, "saved", save_request(tmp_path), second)
    serve.serve(synthesizer, lines, events.append)
    assert {"event": "save-cancelled", "id": 9} in events
    assert events[-1]["event"] == "saved" and events[-1]["id"] == 10
    assert [p.name for p in tmp_path.iterdir()] == ["b.wav"]


def test_a_translated_save_is_translated_whole_then_read_in_that_language(tmp_path, fake_soundfile):
    synthesizer, events = FakeSynthesizer(), []
    request = save_request(tmp_path, translate="es", voice="ef_dora")
    serve.serve(synthesizer, then_wait(events, "saved", request), events.append, FakeTranslator)
    said = " ".join(text for text, _, _ in synthesizer.rendered)
    assert said == "[es] One sentence. And another one."
    assert {(voice, language) for _, voice, language in synthesizer.rendered} == {("ef_dora", "es")}


@pytest.mark.parametrize(
    ("change", "message"),
    [
        ({"output": None}, "needs an output file"),
        ({"output": "/tmp/a.aiff"}, "saved as one of"),
        ({"subtitles": "/tmp/a.txt"}, ".srt or .vtt"),
        ({"text": "  "}, "needs text"),
    ],
)
def test_a_save_that_cant_be_done_is_refused_before_it_starts(tmp_path, change, message):
    _, events = run(save_request(tmp_path) | change)
    assert events[0]["event"] == "error" and message in events[0]["message"]
    assert list(tmp_path.iterdir()) == []


def test_the_lexicon_is_read_again_and_the_next_reading_respells_with_it():
    lexicons = iter([Lexicon({"kubectl": "cube control"})])
    synthesizer, events = run(
        {"op": "lexicon"},
        {"op": "speak", "id": 1, "text": "end"},
        lexicons=lambda: next(lexicons),
    )
    assert synthesizer.lexicon.apply("kubectl") == "cube control"
    assert events[0]["event"] == "started"


def test_a_lexicon_that_wont_parse_is_an_error_and_the_old_one_stays():
    def broken():
        raise ValueError("lexicon.toml isn't valid TOML")

    synthesizer = FakeSynthesizer()
    synthesizer.lexicon = Lexicon({"a": "b"})
    _, events = run({"op": "lexicon", "id": 4}, lexicons=broken, synthesizer=synthesizer)
    assert events == [{"event": "error", "id": 4, "message": "lexicon.toml isn't valid TOML"}]
    assert synthesizer.lexicon.words == {"a": "b"}


class FakeTranslator:
    def __init__(self):
        self.asked = []

    def stream(self, text, language):
        self.asked.append((text, language))
        yield f"[{language}] {text}"


def test_a_translation_is_asked_for_and_each_piece_is_announced():
    translator = FakeTranslator()
    synthesizer, events = run(
        {"op": "speak", "id": 3, "text": "end", "translate": "es"},
        translators=lambda: translator,
    )
    assert translator.asked == [("end", "es")]
    assert synthesizer.translated == ["[es] end"]
    assert {"event": "translated", "id": 3, "text": "[es] end", "start": 0.0, "end": 1.0} in events


class PieceTranslator:
    """One answer per piece, the second empty, as a model may give."""

    def stream(self, text, language):
        from vino_speak.text import chunks
        from vino_speak.translate import MAX_PIECE_CHARS

        for n, _ in enumerate(chunks(text, MAX_PIECE_CHARS)):
            yield "" if n == 1 else f"piece {n}"


def test_each_translated_piece_is_sent_with_where_it_lies():
    sentence = "This sentence is here to be about one hundred characters long so the piece sizes are easy. "
    text = sentence * 40
    synthesizer, events = run(
        {"op": "speak", "id": 4, "text": text, "translate": "es"},
        {"op": "stop"},
        translators=PieceTranslator,
    )
    from vino_speak.translate import spans

    sent = [e for e in events if e["event"] == "translated"]
    where = spans(text)
    assert [e["text"] for e in sent] == [f"piece {n}" for n in range(len(where)) if n != 1]
    assert (sent[0]["start"], sent[0]["end"]) == (0.0, round(where[0][1], 4))
    assert (sent[1]["start"], sent[1]["end"]) == tuple(round(x, 4) for x in where[2])
    assert synthesizer.translated[1] == "", "an empty piece is still passed on, unannounced"


def test_without_translate_no_translator_is_made():
    def never():
        raise AssertionError("made a translator")

    _, events = run({"op": "speak", "id": 1, "text": "end"}, translators=never)
    assert events[-1]["event"] == "done"


def test_a_translation_the_worker_cant_make_is_an_error_for_that_reading():
    def missing():
        raise ValueError("The translation model isn't downloaded.")

    _, events = run({"op": "speak", "id": 4, "text": "x", "translate": "es"}, translators=missing)
    assert events == [
        {"event": "error", "id": 4, "message": "The translation model isn't downloaded."}
    ]


def test_translate_must_be_a_language_or_null():
    _, events = run({"op": "speak", "id": 4, "text": "x", "translate": 3})
    assert events[0]["message"] == "translate must be a language, or null."


def test_the_translator_is_made_once_and_a_failed_make_is_tried_again():
    made = []

    def make():
        made.append(1)
        if len(made) == 1:
            raise ValueError("not yet")
        return "translator"

    get = serve.once(make)
    with pytest.raises(ValueError):
        get()
    assert get() == get() == "translator"
    assert len(made) == 2


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
    assert synthesizer.requests[0] == ("A long read.", "af_heart", None, 1.0, "hdmi_sink")


def test_a_volume_changes_the_reading_playing_without_stopping_it():
    synthesizer, events = run(
        {"op": "speak", "id": 1, "text": "A long read.", "volume": 0.8},
        {"op": "volume", "volume": 0.25},
    )
    assert synthesizer.speeches[0].volume == 0.25
    assert by_id(events, 1) == ["started", "first-audio", "done"]
    assert events[-1] == {"event": "done", "id": 1, "stopped": True}, "stopped by stdin's end"


def test_a_volume_with_nothing_playing_is_no_error():
    _, events = run({"op": "volume", "volume": 0.5})
    assert events == []


@pytest.mark.parametrize("volume", [0, 1, 0.5])
def test_a_volume_from_0_to_1_is_taken(volume):
    synthesizer, events = run({"op": "speak", "id": 1, "text": "end", "volume": volume})
    assert synthesizer.speeches[0].volume == volume
    assert by_id(events, 1) == ["started", "first-audio", "done"]


@pytest.mark.parametrize("volume", [-0.01, 1.01, "loud", True])
def test_a_volume_outside_0_to_1_is_refused(volume):
    synthesizer, events = run(
        {"op": "speak", "id": 1, "text": "end", "volume": volume},
        {"op": "volume", "id": 2, "volume": volume},
    )
    assert synthesizer.speeches == []
    assert [(e["event"], e["id"]) for e in events] == [("error", 1), ("error", 2)]
    assert "from 0 to 1" in events[0]["message"]


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
    monkeypatch.setattr(
        cli, "Synthesizer", lambda device, model_dir, lexicon: loaded.append(device)
    )
    monkeypatch.setattr(cli.serve, "main", lambda make, translators, lexicons: make())
    cli.main(["--serve", "--device", "CPU"])
    assert loaded == ["CPU"]


def test_cli_serve_makes_a_translator_only_once_its_model_is_downloaded(monkeypatch):
    made = {}
    monkeypatch.setattr(
        cli.serve, "main", lambda make, translators, lexicons: made.update(t=translators)
    )
    cli.main(["--serve", "--device", "CPU"])
    monkeypatch.setattr(cli.translate, "is_downloaded", lambda: False)
    with pytest.raises(ValueError, match="isn't downloaded"):
        made["t"]()
    monkeypatch.setattr(cli.translate, "is_downloaded", lambda: True)
    translator = made["t"]()
    assert translator.device == "CPU" and not translator.loaded


@pytest.mark.parametrize(
    "argv", [["Hi"], ["--file", "a.md"], ["--print-text"], ["--output", "a.wav"]]
)
def test_cli_serve_takes_nothing_else(argv):
    with pytest.raises(SystemExit) as exit_info:
        cli.main(["--serve", *argv])
    assert exit_info.value.code == 2


def bare_reading(reading_id, offset, events):
    """A Reading with no speech, from `offset` into its text."""
    reading = serve.Reading.__new__(serve.Reading)
    reading.id, reading.offset, reading.shown, reading.emit = (
        reading_id,
        offset,
        offset,
        events.append,
    )
    return reading


def test_progress_is_sent_in_steps_up_to_the_end():
    events = []
    reading = bare_reading(4, 0.0, events)
    for done in (0.001, 0.004, 0.006, 0.0062, 0.5, 0.998, 1.0, 1.0):
        serve.Reading._progress(reading, done)
    assert [e["done"] for e in events] == [0.006, 0.5, 0.998, 1.0]
    assert all(e == {"event": "progress", "id": 4, "done": e["done"]} for e in events)


def test_a_reading_from_the_middle_starts_at_its_sentence():
    text = "First one. Second one. Third one."
    synthesizer, events = run({"op": "speak", "id": 5, "text": text, "from": 0.5}, {"op": "stop"})
    assert synthesizer.requests[0][0] == "Second one. Third one."
    assert serve.resumed(text, {}) == (text, 0.0)
    assert serve.resumed(text, {"from": 0.05}) == (text, 0.0)
    assert serve.resumed(text, {"from": 0.9}) == ("Third one.", 23 / 33)


def test_its_progress_and_pieces_are_placed_in_the_whole_text():
    translator = FakeTranslator()
    text = "First one. Second one. Third one!"
    synthesizer, events = run(
        {"op": "speak", "id": 6, "text": text, "from": 0.9, "translate": "es"},
        translators=lambda: translator,
    )
    assert translator.asked == [("Third one!", "es")]
    piece = next(e for e in events if e["event"] == "translated")
    assert (piece["start"], piece["end"]) == (round(23 / 33, 4), 1.0)
    sent = []
    reading = bare_reading(6, 0.5, sent)
    for done in (0.0, 0.5, 1.0):
        serve.Reading._progress(reading, done)
    assert [e["done"] for e in sent] == [0.75, 1.0]


@pytest.mark.parametrize("at", [1, 1.5, -0.1, True, "0.5"])
def test_a_from_outside_0_up_to_1_is_refused(at):
    _, events = run({"op": "speak", "id": 7, "text": "Hi.", "from": at})
    assert events == [
        {"event": "error", "id": 7, "message": "from must be a number from 0 up to 1."}
    ]


def test_ssml_is_read_only_from_the_start():
    ssml = "<speak>One. Two.</speak>"
    _, events = run({"op": "speak", "id": 8, "text": ssml, "from": 0.5})
    assert events[-1]["message"] == "SSML can only be read from the start."


def test_pause_and_resume_hold_the_reading_without_ending_it():
    synthesizer, events = run(
        {"op": "pause"},  # with nothing playing, nothing to hold
        {"op": "speak", "id": 9, "text": "Hello."},
        {"op": "pause"},
    )
    assert synthesizer.speeches[0].paused
    assert [e["event"] for e in events] == ["started", "first-audio", "done"], (
        "a pause ends nothing"
    )
    synthesizer, _ = run(
        {"op": "speak", "id": 9, "text": "Hello."}, {"op": "pause"}, {"op": "resume"}
    )
    assert synthesizer.speeches[0].paused is False
    assert len(synthesizer.speeches) == 1
