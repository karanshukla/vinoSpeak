# vinospeak-gui (proof of concept)

Find a file with [vinoSearch](https://github.com/karanshukla/vinoSearch), see it
as vinoSpeak will say it, and listen. Searches through vinoSearch's runner on
D-Bus (the GNOME search provider, which it serves on any desktop) and speaks
through the `vinospeak` CLI, so it never loads a model itself.

```bash
cargo run --release                                    # vinospeak from PATH or ~/.local/bin
VINOSPEAK=../.venv/bin/vinospeak cargo run --release   # a checkout's vinospeak
```

Text files only for now: PDFs and other documents show up but can't be read
aloud until vinoSpeak can get their text from vinoSearch.

Checked without anyone at the screen: `VINOSPEAK_GUI_QUERY` searches,
`VINOSPEAK_GUI_SCREENSHOT=out.png` saves the window once the first hit's text is
in and exits, and `VINOSPEAK_GUI_SPEAK=1` starts reading first.
