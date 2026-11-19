import sys
from types import ModuleType

import numpy as np
import pytest
from conftest import FakeSoundFile

from vino_speak import save, script
from vino_speak.tts import Audio


class FakeSynthesizer:
    """A second of silence per call, or `fail_on` raised for that text."""

    def __init__(self, rate=24000, fail_on=None, lexicon=None):
        self.rate = rate
        self.fail_on = fail_on
        self.lexicon = lexicon
        self.calls = []

    def synthesize(self, text, voice="af_heart", language=None, speed=1.0):
        if text == self.fail_on:
            raise RuntimeError("synthesis failed")
        self.calls.append((text, voice, language, speed))
        return Audio(np.zeros(self.rate, dtype=np.float32), self.rate)

    def plan(self, text, voice="af_heart", language=None, speed=1.0, by_sentence=False):
        return script.plan(text, voice, language, speed, self.lexicon, by_sentence)

    def render(self, step):
        if not step.text:
            return Audio(np.zeros(round(step.silence * self.rate), np.float32), self.rate)
        return self.synthesize(step.text, step.voice, step.language, step.speed)


@pytest.fixture(autouse=True)
def fake_soundfile(monkeypatch):
    FakeSoundFile.opened.clear()
    soundfile = ModuleType("soundfile")
    soundfile.SoundFile = FakeSoundFile
    monkeypatch.setitem(sys.modules, "soundfile", soundfile)


@pytest.mark.parametrize(
    ("name", "saved_as"),
    [
        ("a.wav", b"WAV/PCM_16"),
        ("a.flac", b"FLAC/PCM_16"),
        ("a.ogg", b"OGG/VORBIS"),
        ("a.opus", b"OGG/OPUS"),
        ("a.MP3", b"MP3/MPEG_LAYER_III"),
    ],
)
def test_each_suffix_is_saved_in_its_own_format(tmp_path, name, saved_as):
    save.save(FakeSynthesizer(), "Hello.", tmp_path / name, "af_heart", None, 1.0)
    assert (tmp_path / name).read_bytes() == saved_as


@pytest.mark.parametrize("name", ["a.aiff", "a", "a.txt"])
def test_a_suffix_that_names_no_format_is_refused(name, tmp_path):
    with pytest.raises(ValueError, match="saved as one of"):
        save.check_output(tmp_path / name)


def test_the_text_is_saved_a_chunk_at_a_time_with_progress(tmp_path):
    synthesizer = FakeSynthesizer()
    progress = []
    text = "First. " + "A longer sentence that keeps going on. " * 6

    seconds = save.save(
        synthesizer,
        text,
        tmp_path / "a.wav",
        "bm_george",
        "en-gb",
        1.2,
        None,
        lambda done, total: progress.append((done, total)),
    )

    calls = len(synthesizer.calls)
    assert calls > 1
    assert synthesizer.calls[0] == ("First.", "bm_george", "en-gb", 1.2)
    assert progress == [(n, calls) for n in range(1, calls + 1)]
    assert seconds == calls
    assert len(FakeSoundFile.opened[0].writes) == calls


def test_with_subtitles_each_sentence_is_one_call_and_one_cue(tmp_path):
    synthesizer = FakeSynthesizer()
    subs = tmp_path / "a.srt"

    save.save(synthesizer, "One. Two. Three.", tmp_path / "a.wav", "af_heart", None, 1.0, subs)

    assert [call[0] for call in synthesizer.calls] == ["One.", "Two.", "Three."]
    assert "00:00:02,000 --> 00:00:03,000\nThree." in subs.read_text(encoding="utf-8")


def test_subtitles_show_the_words_as_written_not_as_respelled(tmp_path):
    from vino_speak.lexicon import Lexicon

    synthesizer = FakeSynthesizer(lexicon=Lexicon({"kubectl": "cube control"}))
    subs = tmp_path / "a.srt"

    save.save(synthesizer, "Run kubectl. Done.", tmp_path / "a.wav", "af_heart", None, 1.0, subs)

    assert synthesizer.calls[0][0] == "Run cube control."
    cues = subs.read_text(encoding="utf-8")
    assert "Run kubectl." in cues and "cube" not in cues


def test_a_failed_save_leaves_neither_the_file_nor_its_partial(tmp_path):
    with pytest.raises(RuntimeError, match="synthesis failed"):
        save.save(
            FakeSynthesizer(fail_on="Two."),
            "One. Two.",
            tmp_path / "a.wav",
            "af_heart",
            None,
            1.0,
            tmp_path / "a.srt",
        )
    assert list(tmp_path.iterdir()) == []


def test_speech_at_another_rate_is_refused(tmp_path):
    with pytest.raises(RuntimeError, match="16000 Hz"):
        save.save(FakeSynthesizer(rate=16000), "Hi.", tmp_path / "a.wav", "af_heart", None, 1.0)


def test_bad_subtitles_are_refused_before_any_synthesis(tmp_path):
    synthesizer = FakeSynthesizer()
    with pytest.raises(ValueError, match="srt or"):
        save.save(synthesizer, "Hi.", tmp_path / "a.wav", "af_heart", None, 1.0, tmp_path / "a.txt")
    assert synthesizer.calls == []


def test_a_pause_is_silence_with_no_cue_and_moves_the_next_one_on(tmp_path):
    subs = tmp_path / "a.vtt"
    ssml = '<speak>One.<break time="500ms"/>Two.</speak>'

    seconds = save.save(FakeSynthesizer(), ssml, tmp_path / "a.wav", "af_heart", None, 1.0, subs)

    assert seconds == 2.5
    assert len(FakeSoundFile.opened[0].writes[1]) == 12000
    assert "00:00:01.500 --> 00:00:02.500\nTwo." in subs.read_text(encoding="utf-8")
