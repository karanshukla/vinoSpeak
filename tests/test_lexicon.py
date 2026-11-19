import pytest

from vino_speak import lexicon
from vino_speak.lexicon import Lexicon


def test_a_lowercase_entry_matches_any_case_and_a_capitalised_one_only_its_own():
    words = Lexicon({"kubectl": "cube control", "NGINX": "engine x"})
    assert words.apply("Kubectl, KUBECTL") == "cube control, cube control"
    assert words.apply("NGINX not nginx") == "engine x not nginx"


def test_only_whole_words_are_respelled():
    words = Lexicon({"sql": "sequel"})
    assert words.apply("sql mysql sqlite sql.") == "sequel mysql sqlite sequel."


def test_the_longest_entry_wins_and_nothing_is_respelled_twice():
    words = Lexicon({"Dr.": "Doctor", "Dr. Who": "Doctor Who", "Doctor": "Doc"})
    assert words.apply("Dr. Who met Dr. Hale.") == "Doctor Who met Doctor Hale."


def test_an_empty_lexicon_leaves_the_text_alone():
    assert Lexicon().apply("Anything.") == "Anything."


def test_parse_reads_the_words_table():
    words = lexicon.parse('[words]\nkubectl = "cube control"\n"Dr. Who" = "Doctor Who"\n')
    assert words.words == {"kubectl": "cube control", "Dr. Who": "Doctor Who"}


@pytest.mark.parametrize(
    ("source", "complaint"),
    [
        ("[words", "isn't valid TOML"),
        ("words = 3", "must be a table"),
        ("[words]\nkubectl = 3", "respelling in quotes"),
        ('[words]\n" " = "x"', "respelling in quotes"),
    ],
)
def test_a_bad_lexicon_says_what_is_wrong(source, complaint):
    with pytest.raises(ValueError, match=complaint):
        lexicon.parse(source)


def test_a_lexicon_of_the_cap_is_read_and_one_more_is_refused():
    def source(n):
        return "[words]\n" + "".join(f'w{i} = "x"\n' for i in range(n))

    assert len(lexicon.parse(source(lexicon.MAX_ENTRIES)).words) == lexicon.MAX_ENTRIES
    with pytest.raises(ValueError, match="the most it takes"):
        lexicon.parse(source(lexicon.MAX_ENTRIES + 1))


def test_the_default_lexicon_is_used_when_it_exists(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CONFIG_HOME", str(tmp_path))
    assert lexicon.load().words == {}
    (tmp_path / "vinospeak").mkdir()
    (tmp_path / "vinospeak" / "lexicon.toml").write_text('[words]\nsql = "sequel"\n')
    assert lexicon.load().words == {"sql": "sequel"}


def test_a_lexicon_asked_for_must_exist(tmp_path):
    with pytest.raises(ValueError, match="Can't read the lexicon"):
        lexicon.load(tmp_path / "missing.toml")
