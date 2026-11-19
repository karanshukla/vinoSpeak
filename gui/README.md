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

On Fedora Atomic or openSUSE Aeon you don't need cargo: the top-level
installer's `--gui` fetches the release's prebuilt binary
(`curl -fsSL https://github.com/karanshukla/vinoSpeak/releases/latest/download/install.sh | bash -s -- --gui`).
To build from a checkout there, run `install.sh` inside a toolbox or distrobox
that has cargo: the binary goes to `~/.cargo/bin` and the menu entry to
`~/.local/share`, both of which the host sees. Use a container of the host's
own release (what `toolbox create` gives you; on openSUSE Aeon, a distrobox with
a Tumbleweed image), since a binary linked against a newer glibc than the
host's won't start there.

vinoSearch is optional. Without it the window lists vinoWhisper's sessions
from both its native folder (`~/.local/share/vinowhisper/transcripts`) and its
Flatpak's (`~/.var/app/io.github.karanshukla.vinowhisper/data/...`), opens any
text file through the folder button, a drop on the window or
`vinospeak-gui FILE`, and turns search off. With it, those sessions join its
recent files.

Text files are read by vinospeak itself; PDFs and other documents through the
text vinoSearch extracts. A vinoWhisper session is titled by its summary (or the
time it was recorded) and has a Summary / Transcript toggle; Summary is
disabled until vinoWhisper has summarised it. Spreadsheets and source code are
left out of the lists, though vinoSearch indexes them: `vinospeak --file` still
reads one if you want it.

The voice, speed and volume are kept between runs in
`~/.config/vinospeak/gui.json` (or under `$XDG_CONFIG_HOME`). Mute is not:
the window always opens unmuted. Volume is the reading's own, not the
desktop's, and a change is heard mid-reading.

Checked without anyone at the screen: `VINOSPEAK_GUI_QUERY` searches,
`VINOSPEAK_GUI_SCREENSHOT=out.png` saves the window once the first hit's text is
in and exits, and `VINOSPEAK_GUI_SPEAK=1` starts reading first (a number
above 1 reads that many seconds before the screenshot, rather than 4).
`VINOSPEAK_GUI_ABOUT=1` opens the About page, and `=delete` opens it at the
model's delete confirmation.

About's Translation row downloads SmolLM3-3B (1.7 GB, `vinospeak
--download-translation-model`) after you confirm, and deletes it again,
unless vinoWhisper uses the same copy. Until it is there the player's
Translate… button opens that row; once it is, Translate is a picker: pick a
language and Read aloud translates the file into it as it reads, in a voice
of that language. The voice picker then lists only that language's voices.

The player is two rows that never reflow: the transport (Read aloud or Stop,
what it is doing, the volume) and the settings for the next reading
(Translate, Voice, Speed, Output) under their headings, four across or two by
two in a narrow window.
The worker loads the model only for the first translated reading. Until the
first audio plays, the player shows a spinner with Starting… (or
Translating…), since loading the models and translating the first sentence
take seconds.

About also shows the downloaded model's size, with Delete… to remove it
(`vinospeak --delete-model`). It waits for a reading to stop, and stops the
`vinospeak --serve` worker first; the next reading downloads the model again.

`cargo test` drives the window headless with
[egui_kittest](https://crates.io/crates/egui_kittest), finding widgets by their
AccessKit labels, against a fake vinoSearch and a fake vinospeak script.
