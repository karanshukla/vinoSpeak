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
