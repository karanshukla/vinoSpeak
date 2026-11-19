import pytest

from vino_speak import script
from vino_speak.lexicon import Lexicon
from vino_speak.script import Step


def test_plain_text_is_planned_in_chunks_with_the_voice_asked_for():
    steps = script.plan("Hello. Again.", "bm_george", "en-gb", 1.2)
    assert steps[0] == Step("Hello.", "bm_george", "en-gb", 1.2)


def test_by_sentence_gives_a_step_a_sentence():
    assert [
        s.text for s in script.plan("One. Two. Three.", "af_heart", None, 1.0, by_sentence=True)
    ] == [
        "One.",
        "Two.",
        "Three.",
    ]


def test_the_lexicon_respells_text_but_not_markup():
    words = Lexicon({"break": "brake", "sql": "sequel"})
    steps = script.plan('<speak>sql<break time="1s"/>break</speak>', "af_heart", None, 1.0, words)
    assert [(s.text, s.silence) for s in steps] == [("sequel", 0.0), ("", 1.0), ("brake", 0.0)]


def test_a_sentence_keeps_its_written_text_beside_its_respelling():
    words = Lexicon({"kubectl": "cube control", "Dr. Who": "Doctor Who"})
    text = "Ask Dr. Who. Run kubectl. Done."
    by_sentence = script.plan(text, "af_heart", None, 1.0, words, by_sentence=True)
    assert [(s.text, s.written) for s in by_sentence] == [
        ("Ask Doctor Who.", "Ask Dr. Who."),
        ("Run cube control.", "Run kubectl."),
        ("Done.", "Done."),
    ]
    chunked = script.plan(text, "af_heart", None, 1.0, words)
    assert " ".join(s.text for s in chunked) == "Ask Doctor Who. Run cube control. Done."


def test_a_zero_break_adds_no_step():
    steps = script.plan('<speak>A<break strength="none"/>B</speak>', "af_heart", None, 1.0)
    assert [s.text for s in steps] == ["A", "B"]


def test_an_ssml_voice_brings_its_own_language_unless_one_is_set():
    steps = script.plan(
        '<speak><voice name="ef_dora">Hola.</voice><lang xml:lang="it"><voice name="ef_dora">Ciao.</voice></lang></speak>',
        "af_heart",
        "en-us",
        1.0,
    )
    assert [(s.voice, s.language) for s in steps] == [("ef_dora", None), ("ef_dora", "it")]


def test_a_prosody_rate_past_the_range_is_held_at_its_edge():
    steps = script.plan(
        '<speak><prosody rate="x-fast">Quick.</prosody></speak>', "af_heart", None, 1.9
    )
    assert steps[0].speed == 2.0
    steps = script.plan(
        '<speak><prosody rate="x-slow">Slow.</prosody></speak>', "af_heart", None, 0.9
    )
    assert steps[0].speed == 0.5
    steps = script.plan(
        '<speak><prosody rate="x-slow">Inside.</prosody></speak>', "af_heart", None, 1.2
    )
    assert steps[0].speed == pytest.approx(0.6)


def test_ssml_with_nothing_to_say_is_refused():
    with pytest.raises(ValueError, match="no text to speak"):
        script.plan('<speak><break time="1s"/></speak>', "af_heart", None, 1.0)


def test_spoken_shows_the_text_and_each_pause():
    steps = script.plan('<speak>A.<break time="1500ms"/>B.</speak>', "af_heart", None, 1.0)
    assert script.spoken(steps) == "A.\n[pause 1.5 s]\nB."
