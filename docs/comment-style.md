# Comment style

A comment is the last resort. Work down this ladder, and write prose only
when all four rungs fail.

1. **Extract it.** Logic that a comment explains belongs in a function with
   a domain name. Arithmetic spelled out in a comment belongs in named
   constants. A coercion that is explained twice belongs in one named helper.

   ```rust
   // Before
   // Not selectable: a press on them drags the window.
   ui.add(egui::Label::new(title).selectable(false));

   // After
   ui.add(drags_the_window(title));
   ```

2. **Name it.** A comment that labels a block (`# the download is under
   way`) means the block wants to be a function with that name
   (`delete_mid_download`). Sentinels and magic sets get names too:
   `NOT_USED_BY_THE_PIPELINE = {...}`, not `SKIP = {...}  # never used`.

3. **Pin it with a test pair.** A comment that states a business rule is a
   rule nothing enforces. Write one test inside the boundary and one
   outside, named after the rule (`test_a_part_at_the_cap_is_read_and_one_byte_over_is_refused`),
   instead of writing `# exactly at the cap is fine`. The pair is the point:
   it fails the day someone moves the limit. In a test, a comment that labels
   the next few asserts is the same smell. Split those asserts into their
   own test, named after what the comment said.

4. **Pin cross-boundary rules with an end-to-end test.** The `--serve`
   JSON lines, what `vinospeak --print-text` prints for a document, and what
   `gui/install.sh` writes belong in `tests/test_serve.py`,
   `tests/test_cli.py` or `tests/test_gui_install.py`, not in a paragraph.

5. **Only then, prose.** Hidden constraints, upstream bugs, protocol
   requirements, incident history that still constrains the code. State the
   constraint, not its biography: "KWin can stall a hidden window with
   vsync" is enough without the story of how we found out. Measured claims
   carry a date. The longer why goes in [architecture.md](architecture.md).

## Linking the test from the code

When rung 3 or 4 replaced a comment, the code it governs says which test
pins the rule, always with the phrase "pinned by":

```python
# vinoWhisper's own read limits (pinned by
# `test_session.py::test_sixteen_entries_are_read_and_seventeen_are_refused`).
MAX_ENTRIES = 16
```

```rust
/// How many recent files fill the list before a search (pinned by
/// `app::tests::the_recent_list_stops_at_twenty`).
const RECENT_LISTED: usize = 20;
```

Two checks (CI job "Doc Links") resolve these:

- `tests/test_doc_links.py`: every `` `test_file.py::test_name` `` in
  `vino_speak/` or `scripts/` must be a `def test_name(` in `tests/test_file.py`.
- `gui/tests/doc_links.rs`: every `` `tests::name` `` or
  `` `module::tests::name` `` in `gui/src/` must be a `#[test]` fn, in that
  file or the module's `tests.rs` (`app::tests::name` is in
  `gui/src/app/tests.rs`). rustdoc can't link to `#[test]` functions, so
  these are code spans, not intra-doc links.

## Exempt

- Docstrings, and `///` and `//!` docs on items, which say what an item is
  and how to call it. They follow the same "constraint, not biography" rule,
  and shouldn't restate the signature, but aren't replaced by code.
- `# noqa` and `# type: ignore` reasons, `#[allow]` reasons, `// SAFETY:`
  comments, and coverage pragmas.
