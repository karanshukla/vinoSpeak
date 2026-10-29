# vinospeak-gui (proof of concept)

Find a file with [vinoSearch](https://github.com/karanshukla/vinoSearch), see it
as vinoSpeak will say it, and listen. Searches through vinoSearch's runner on
D-Bus (the GNOME search provider, which it serves on any desktop) and speaks
through the `vinospeak` CLI, so it never loads a model itself.

```bash
cargo run --release                                    # vinospeak from PATH or ~/.local/bin
VINOSPEAK=../.venv/bin/vinospeak cargo run --release   # a checkout's vinospeak
VINOSPEAK=../.venv/bin/vinospeak ./install.sh          # into the app menu; --uninstall removes it
```

On Fedora Atomic or openSUSE Aeon, run `install.sh` inside a toolbox or
distrobox that has cargo: the binary goes to `~/.cargo/bin` and the menu entry
to `~/.local/share`, both of which the host sees. Use a container of the host's
own release (what `toolbox create` gives you), since a binary linked against a
newer glibc than the host's won't start there.

Text files are read by vinospeak itself; PDFs and other documents through the
text vinoSearch extracts. A vinoWhisper session is titled by its summary (or the
time it was recorded) and has a Summary / Transcript toggle; Summary is
disabled until vinoWhisper has summarised it.

Checked without anyone at the screen: `VINOSPEAK_GUI_QUERY` searches,
`VINOSPEAK_GUI_SCREENSHOT=out.png` saves the window once the first hit's text is
in and exits, and `VINOSPEAK_GUI_SPEAK=1` starts reading first.

`cargo test` drives the window headless with
[egui_kittest](https://crates.io/crates/egui_kittest), finding widgets by their
AccessKit labels, against a fake vinoSearch and a fake vinospeak script.
