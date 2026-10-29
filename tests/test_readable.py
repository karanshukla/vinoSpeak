import time

import pytest

from vino_speak.readable import CODE_SKIPPED, document, markdown, plain, read


def spoken(text: str) -> str:
    return " ".join(markdown(text).split())


def test_headings_and_list_items_become_sentences():
    assert spoken("# Install\n\n- one\n- two!\n") == "Install. one. two!"


def test_a_wrapped_list_item_ends_once():
    assert spoken("- pinned (a,\n  b) here\n- next") == "pinned (a, b) here. next."


def test_setext_headings():
    assert spoken("Title\n=====\n\nBody") == "Title. Body"


def test_code_blocks_are_skipped_and_inline_code_is_kept():
    text = "Run `uv sync`:\n\n```bash\nrm -rf /\n```\n\nDone."
    assert spoken(text) == f"Run uv sync: {CODE_SKIPPED} Done."


def test_an_unclosed_fence_skips_to_the_end():
    assert spoken("Before.\n\n```\ncode\nmore") == f"Before. {CODE_SKIPPED}"


def test_links_keep_their_text_and_images_go():
    text = "[![badge](https://b.svg)](https://ci) See [docs](https://x.dev) and ![a dog](d.png)."
    assert spoken(text) == "See docs and ."


def test_urls_are_said_as_their_host():
    assert (
        plain("Go to https://www.github.com/a/b?c=1, then stop.") == "Go to github.com, then stop."
    )
    assert spoken("<https://pypi.org/p/x>") == "pypi.org"


def test_reference_links_and_definitions():
    assert spoken("Read [the spec][1].\n\n[1]: https://spec.example\n") == "Read the spec."


def test_emphasis_goes_but_snake_case_stays():
    assert (
        spoken("**bold** *it* _also_ ~~gone~~ keep_this_name") == "bold it also gone keep_this_name"
    )


def test_tables_are_read_row_by_row():
    text = "| Device | Time |\n|---|:-:|\n| NPU | 1.5 s |\n"
    assert spoken(text) == "Device, Time. NPU, 1.5 s."


def test_html_front_matter_comments_and_rules_go():
    text = '---\ntitle: x\n---\n<p align="center"><img src="a.svg"></p>\n<!-- hidden -->\n***\nText'
    assert spoken(text) == "Text"


def test_quotes_and_escapes():
    assert spoken("> quoted \\*not emphasis\\*") == "quoted *not emphasis*"


def test_nested_emphasis_and_parenthesised_link_targets():
    assert spoken("**bold *and it* too** __x _y_ z__") == "bold and it too x y z"
    assert spoken("See [Kokoro](https://en.wikipedia.org/wiki/Kokoro_(novel)).") == "See Kokoro."


# Each of these is unclosed markup repeated along one line. Before the inline
# patterns stopped at the next opening delimiter, 5,000 `**a ` took 3.3 s and
# the time grew with the square of the length, so a large .md file could hang
# the preview in the window (2026-10-25). Now 25,000 take about 20 ms.
@pytest.mark.parametrize(
    "unit", ["**a ", "__a ", "*a ", "_a ", "[", "![", "[a", "[^a", "[a](", "<http://a"]
)
def test_unclosed_inline_markup_is_linear(unit: str) -> None:
    text = unit * 25_000
    start = time.perf_counter()
    markdown(text)
    assert time.perf_counter() - start < 1.0


def test_a_document_line_without_punctuation_ends_a_sentence():
    text = "Introduction\nThe guild grew this year.\nIt hired keepers"
    assert document(text) == "Introduction.\nThe guild grew this year.\nIt hired keepers."


def test_a_docx_table_is_read_cell_by_cell():
    # vinoSearch's DOCX extractor: one line per paragraph and per table cell
    assert " ".join(document("Results\nName\nScore\nAlice\n90\n").split()) == (
        "Results. Name. Score. Alice. 90."
    )


def test_a_word_hyphenated_at_a_line_end_is_rejoined():
    assert document("more infor-\nmation here.") == "more information here."
    assert document("more infor- \n  mation here.") == "more information here."
    assert document("COVID-\n19 cases.") == "COVID-19 cases."
    assert document("Jean-\nPaul came.") == "Jean-Paul came."


def test_an_all_caps_heading_is_left_as_it_is():
    # Kokoro said "ANNUAL REPORT" and "Annual Report" alike (2026-10-29, CPU)
    assert document("ANNUAL REPORT\nBody.") == "ANNUAL REPORT.\nBody."


def test_a_document_url_is_its_host_and_an_email_is_kept():
    text = "See https://example.org/reports/2025\nWrite to guild@example.org"
    assert document(text) == "See example.org.\nWrite to guild@example.org."


def test_a_blank_line_stays_a_paragraph_break():
    assert document("Page one\n\nPage two") == "Page one.\n\nPage two."


def test_a_wrapped_text_file_carries_its_sentences_over_lines():
    text = "A hard-wrapped line\ncarries on here\nAnd a Title\n\nNext"
    assert (
        document(text, wrapped=True)
        == "A hard-wrapped line\ncarries on here.\nAnd a Title.\n\nNext."
    )


@pytest.mark.parametrize(
    ("name", "kind", "spoken"),
    [
        ("a.md", None, "Title."),
        ("a.txt", None, "# Title."),
        ("a.md", "plain", "# Title"),
        ("a.txt", "document", "# Title."),
        ("a.txt", "markdown", "Title."),
    ],
)
def test_read_picks_the_kind_by_suffix_unless_told(tmp_path, name, kind, spoken):
    (tmp_path / name).write_text("# Title")
    assert read(tmp_path / name, kind).strip() == spoken
