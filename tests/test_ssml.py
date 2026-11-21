import pytest

from vino_speak import ssml
from vino_speak.ssml import Pause, Say


def test_only_text_starting_with_speak_is_ssml():
    assert ssml.is_ssml("  <speak>Hi</speak>")
    assert not ssml.is_ssml("I <speak> softly")


def test_plain_ssml_is_one_piece_of_text():
    assert ssml.parse("<speak>Hello   there.</speak>") == [Say("Hello there.")]


def test_a_break_splits_the_text_around_a_pause():
    parts = ssml.parse(
        '<speak>One.<break time="250ms"/>Two.<break strength="strong"/>Three.</speak>'
    )
    assert parts == [Say("One."), Pause(0.25), Say("Two."), Pause(0.75), Say("Three.")]


def test_a_break_with_neither_time_nor_strength_is_medium():
    assert ssml.parse("<speak>A<break/>B</speak>")[1] == Pause(0.5)


def test_a_break_of_ten_seconds_is_kept_and_longer_is_refused():
    assert ssml.parse('<speak>A<break time="10s"/>B</speak>')[1] == Pause(10.0)
    with pytest.raises(ValueError, match="at most 10 s"):
        ssml.parse('<speak>A<break time="10001ms"/>B</speak>')


def test_total_break_duration_and_count_are_capped():
    six = "<speak>A" + '<break time="10s"/>' * 6 + "B</speak>"
    assert sum(p.seconds for p in ssml.parse(six) if isinstance(p, Pause)) == 60.0
    seven = "<speak>A" + '<break time="10s"/>' * 7 + "B</speak>"
    with pytest.raises(ValueError, match="Total <break> time is at most 60 s"):
        ssml.parse(seven)

    too_many = "<speak>A" + '<break time="10ms"/>' * (ssml.MAX_BREAK_COUNT + 1) + "B</speak>"
    with pytest.raises(ValueError, match="at most 256 <break>"):
        ssml.parse(too_many)


@pytest.mark.parametrize("attribute", ['time="soon"', 'strength="huge"'])
def test_a_break_that_is_not_a_duration_is_refused(attribute):
    with pytest.raises(ValueError, match="<break"):
        ssml.parse(f"<speak>A<break {attribute}/>B</speak>")


def test_sub_says_its_alias():
    assert ssml.parse('<speak>The <sub alias="World Wide Web Consortium">W3C</sub>.</speak>') == [
        Say("The World Wide Web Consortium.")
    ]


@pytest.mark.parametrize(
    ("interpret", "said"),
    [("characters", "A P I"), ("spell-out", "A P I"), ("digits", "4 2"), ("date", "API 42")],
)
def test_say_as_spells_out_characters_and_digits(interpret, said):
    parts = ssml.parse(f'<speak><say-as interpret-as="{interpret}">API 42</say-as></speak>')
    expected = {"characters": "A P I 4 2", "spell-out": "A P I 4 2"}.get(interpret, said)
    assert parts == [Say(expected)]


@pytest.mark.parametrize(
    ("rate", "factor"),
    [("slow", 0.75), ("x-fast", 1.5), ("80%", 0.8), ("+20%", 1.2), ("-50%", 0.5), ("1.1", 1.1)],
)
def test_a_prosody_rate_scales_the_speed(rate, factor):
    parts = ssml.parse(f'<speak>A <prosody rate="{rate}">B</prosody> C</speak>')
    assert parts == [Say("A"), Say("B", rate=pytest.approx(factor)), Say("C")]


@pytest.mark.parametrize("rate", ["quick", "-100%", "0"])
def test_a_prosody_rate_that_is_no_speed_is_refused(rate):
    with pytest.raises(ValueError, match="prosody rate"):
        ssml.parse(f'<speak><prosody rate="{rate}">B</prosody></speak>')


def test_nested_rates_multiply():
    parts = ssml.parse(
        '<speak><prosody rate="50%"><prosody rate="x-fast">B</prosody></prosody></speak>'
    )
    assert parts == [Say("B", rate=0.75)]


def test_a_voice_element_names_the_voice_for_its_text():
    parts = ssml.parse('<speak>Me. <voice name="bm_george">George.</voice> Me again.</speak>')
    assert parts == [Say("Me."), Say("George.", voice="bm_george"), Say("Me again.")]


def test_a_voice_element_naming_no_voice_is_refused():
    with pytest.raises(ValueError, match="Kokoro voice"):
        ssml.parse('<speak><voice name="../etc">x</voice></speak>')


def test_xml_lang_sets_the_language_on_speak_or_lang():
    parts = ssml.parse('<speak xml:lang="en-GB">Hello. <lang xml:lang="fr">Bonjour.</lang></speak>')
    assert parts == [Say("Hello.", language="en-gb"), Say("Bonjour.", language="fr-fr")]


def test_a_language_vinospeak_does_not_speak_is_refused():
    with pytest.raises(ValueError, match="isn't a language"):
        ssml.parse('<speak xml:lang="ja">x</speak>')


def test_paragraphs_and_sentences_end_with_a_full_stop():
    parts = ssml.parse("<speak><p>A heading</p><p>Done!</p><s>Then</s></speak>")
    assert parts == [Say("A heading. Done! Then.")]


def test_unknown_elements_are_read_as_their_text():
    assert ssml.parse(
        '<speak>Very <emphasis level="strong">loud</emphasis><mark name="m"/> now.</speak>'
    ) == [Say("Very loud now.")]


def test_a_namespaced_document_is_read_the_same():
    ns = 'xmlns="http://www.w3.org/2001/10/synthesis" version="1.0"'
    assert ssml.parse(f'<speak {ns}>A<break time="1s"/>B</speak>') == [
        Say("A"),
        Pause(1.0),
        Say("B"),
    ]


@pytest.mark.parametrize("text", ["<speak>Fish & chips</speak>", "<speak>open", "<speak/><x/>"])
def test_malformed_xml_is_refused(text):
    with pytest.raises(ValueError, match="well-formed"):
        ssml.parse(text)


def test_a_root_other_than_speak_is_refused():
    with pytest.raises(ValueError, match="one <speak>"):
        ssml.parse("<speaking>x</speaking>")


@pytest.mark.parametrize(
    "text",
    [
        '<!DOCTYPE speak [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;">]><speak>&b;</speak>',
        "<speak><!doctype x></speak>",
        '<!DOCTYPE speak SYSTEM "file:///etc/passwd"><speak>x</speak>',
        "<speak><! ENTITY x 'y'></speak>",
    ],
)
def test_a_dtd_or_entity_is_refused_before_parsing(text, monkeypatch):
    monkeypatch.setattr(ssml.ET, "fromstring", lambda *a: pytest.fail("parsed"))
    with pytest.raises(ValueError, match="DOCTYPE or entities"):
        ssml.parse(text)


def test_the_five_xml_escapes_still_work():
    assert ssml.parse("<speak>Fish &amp; chips &lt;3</speak>") == [Say("Fish & chips <3")]
