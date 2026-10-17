from vino_speak.text import MAX_CHUNK_CHARS, chunks

SENTENCE = "This sentence is here to be about one hundred characters long, so the chunk sizes are easy to read."


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
