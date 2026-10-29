from vino_speak.text import MAX_CHUNK_CHARS, chunks

SENTENCE = "This sentence is here to be about one hundred characters long so the chunk sizes are easy to read."


def test_the_first_sentence_goes_alone_then_chunks_double():
    text = " ".join([SENTENCE] * 7)
    assert [len(c) // len(SENTENCE) for c in chunks(text, max_chars=1000)] == [1, 2, 4]


def test_chunks_never_pass_the_cap():
    text = " ".join([SENTENCE] * 30)
    assert all(len(c) <= MAX_CHUNK_CHARS for c in chunks(text))
    assert " ".join(chunks(text)) == text


def test_short_sentences_are_merged_after_the_first():
    assert chunks("Hi. Yes. No. Maybe so.") == ["Hi.", "Yes. No. Maybe so."]


def test_paragraph_breaks_split_and_whitespace_collapses():
    assert chunks("  A title\n\nThen   the\nbody  ") == ["A title", "Then the body"]


def test_text_without_punctuation_is_one_chunk():
    assert chunks("no punctuation at all here") == ["no punctuation at all here"]


def test_blank_text_has_no_chunks():
    assert chunks(" \n\n ") == []


CLAUSE = "and then it went on past the last clause without stopping"


def test_a_sentence_too_long_for_a_chunk_is_cut_after_its_clauses():
    text = ", ".join([CLAUSE] * 20) + "."
    out = chunks(text)
    assert len(out) > 1
    assert len(out[0]) < 2 * len(CLAUSE), "the first chunk should be about one clause"
    assert all(len(c) <= MAX_CHUNK_CHARS for c in out)
    assert " ".join(out) == text


def test_a_long_run_without_punctuation_is_cut_between_words():
    text = " ".join(["word"] * 500)
    out = chunks(text)
    assert len(out[0]) <= 100
    assert all(len(c) <= MAX_CHUNK_CHARS for c in out)
    assert " ".join(out) == text


def test_a_word_longer_than_a_chunk_is_cut():
    text = "x" * 1000
    out = chunks(text)
    assert all(len(c) <= MAX_CHUNK_CHARS for c in out)
    assert "".join(out) == text


def test_a_word_too_long_for_a_chunk_ends_the_piece_before_it():
    long_word = "x" * (MAX_CHUNK_CHARS + 5)
    assert chunks(f"short {long_word} tail") == [
        "short",
        "x" * MAX_CHUNK_CHARS,
        "xxxxx tail",
    ]


def test_no_cut_after_an_abbreviation_or_an_initial():
    text = "Cache it, e.g. here. Dr. Okafor and J. R. Smith came. The U.S. grew. See fig. 3 vs. 4."
    assert chunks(text) == [
        "Cache it, e.g. here.",
        "Dr. Okafor and J. R. Smith came. The U.S. grew. See fig. 3 vs. 4.",
    ]


def test_a_word_that_ends_like_an_abbreviation_still_ends_its_sentence():
    assert chunks("It was Prof. Stop.") == ["It was Prof.", "Stop."]
    assert chunks("Ask the doctor. Dr. No.") == ["Ask the doctor.", "Dr. No."]
    assert chunks("Do it first. Next step.") == ["Do it first.", "Next step."]


def test_a_blank_line_after_an_abbreviation_still_splits():
    assert chunks("Oil, lamps, etc.\n\nNext") == ["Oil, lamps, etc.", "Next"]


def test_a_long_first_sentence_is_cut_after_its_first_clause_past_sixty_chars():
    first = "Short, then a clause that runs on past sixty characters in all, and then the rest."
    assert chunks(f"{first} Second sentence.") == [
        "Short, then a clause that runs on past sixty characters in all,",
        "and then the rest. Second sentence.",
    ]


def test_a_first_sentence_with_no_late_clause_break_goes_whole():
    long = "A first sentence that runs well past sixty characters without any clause break."
    assert chunks(f"{long} Next.") == [long, "Next."]
    assert chunks("Short, and sweet.") == ["Short, and sweet."]
