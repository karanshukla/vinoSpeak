import time

import pytest

from vino_speak.readable import CODE_SKIPPED, markdown, plain


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
