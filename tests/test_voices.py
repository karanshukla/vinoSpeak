import pytest

from vino_speak import voices


def test_an_id_says_accent_gender_and_name():
    voice = voices.describe("bm_george")
    assert voice == voices.Voice("bm_george", "British English", "male", "en-gb")
    assert voice.name == "George"


@pytest.mark.parametrize("voice_id", ["jf_alpha", "zm_yunjian"])
def test_japanese_and_mandarin_voices_are_not_offered(voice_id):
    assert voices.describe(voice_id) is None
    assert voice_id not in {v.id for v in voices.available()}


@pytest.mark.parametrize("voice_id", ["af", "ax_heart", "../af_heart", "AF_HEART"])
def test_what_is_not_a_voice_id_describes_nothing(voice_id):
    assert voices.describe(voice_id) is None


def test_the_pinned_model_offers_forty_one_voices_grouped_by_accent():
    found = voices.available()
    assert len(found) == 41
    assert [v.id[0] for v in found] == sorted((v.id[0] for v in found), key=list("abefhip").index)
    assert "af_heart" in {v.id for v in found}


def test_a_single_voice_has_all_the_weight():
    assert voices.parse("af_heart") == [("af_heart", 1.0)]


def test_a_blend_normalises_its_weights():
    assert voices.parse("af_heart(3) + bm_george") == [("af_heart", 0.75), ("bm_george", 0.25)]


@pytest.mark.parametrize(
    "spec", ["", "af_heart+", "af_heart(-1)", "af_heart(0)", "af_heart(x)", "../voice"]
)
def test_a_malformed_blend_is_refused(spec):
    with pytest.raises(ValueError):
        voices.parse(spec)


def test_a_blend_of_eight_is_read_and_nine_is_refused():
    assert len(voices.parse("+".join(["af_heart"] * 8))) == 8
    with pytest.raises(ValueError, match="at most 8"):
        voices.parse("+".join(["af_heart"] * 9))


def test_equal_blends_share_one_spelling():
    assert voices.canonical("af_heart(2)+bm_george(2)") == voices.canonical(
        "af_heart(1) + bm_george(1)"
    )
    assert voices.canonical("af_heart") == "af_heart"


@pytest.mark.parametrize(
    ("spec", "language"),
    [("ef_dora", "es"), ("bm_george", "en-gb"), ("pf_dora(1)+af_heart(3)", "pt-br")],
)
def test_a_voice_brings_its_language(spec, language):
    assert voices.language_of(spec) == language


def test_a_voice_of_no_known_accent_brings_no_language():
    assert voices.language_of("xx_custom") is None
