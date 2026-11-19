from pathlib import Path

import pytest

from vino_speak import subtitles
from vino_speak.subtitles import Cue


def test_a_sentence_of_two_lines_is_one_cue_and_longer_is_split():
    two_lines = "word " * 16  # 80 characters: two lines of 42
    assert len(subtitles.split(0.0, 4.0, two_lines.strip())) == 1
    three_lines = "word " * 20
    cues = subtitles.split(0.0, 4.0, three_lines.strip())
    assert len(cues) == 2


def test_a_split_sentences_cues_share_its_time_by_characters_and_end_with_it():
    cues = subtitles.split(1.0, 5.0, " ".join(["word"] * 20))
    assert cues[0].start == 1.0
    assert cues[0].end == cues[1].start
    assert cues[1].end == 5.0
    assert cues[0].end - cues[0].start > cues[1].end - cues[1].start


def test_srt_numbers_its_cues_and_uses_a_comma_before_milliseconds():
    text = subtitles.srt([Cue(0.0, 1.5, "Hello."), Cue(1.5, 3723.25, "Later.")])
    assert text == (
        "1\n00:00:00,000 --> 00:00:01,500\nHello.\n\n2\n00:00:01,500 --> 01:02:03,250\nLater.\n\n"
    )


def test_vtt_has_its_header_and_a_full_stop_before_milliseconds():
    text = subtitles.vtt([Cue(0.0, 1.5, "Hello.")])
    assert text == "WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nHello.\n\n"


def test_a_long_cue_is_wrapped_onto_lines():
    text = subtitles.srt([Cue(0.0, 1.0, " ".join(["word"] * 16))])
    assert all(len(line) <= subtitles.LINE_CHARS for line in text.splitlines())


@pytest.mark.parametrize(("name", "header"), [("out.srt", "1\n"), ("out.VTT", "WEBVTT")])
def test_write_picks_the_format_by_suffix(tmp_path, name, header):
    path = tmp_path / "sub" / name
    subtitles.write(path, [Cue(0.0, 1.0, "Hi.")])
    assert path.read_text(encoding="utf-8").startswith(header)


def test_another_suffix_is_refused():
    with pytest.raises(ValueError, match=r"\.srt or \.vtt"):
        subtitles.check_suffix(Path("out.txt"))
