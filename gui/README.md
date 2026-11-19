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

The voice and blend, speed, volume, Translate, the save format and
Subtitles, and whether the speech API is on are kept between runs in
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
While it reads, the window shows the translation as each piece arrives, with
the piece being spoken tinted; Translation / Original above it switches back
to the file's own text. The translation stays with that file until the next
reading.

The player's rows never reflow: the transport (Read aloud or Stop, what it
is doing, the volume), then the reading's settings (Translate, Voice, Speed,
Output, Blend with, Mix) under their headings, four across or two to a row
in a narrow window, then Save as audio. The volume changes the reading as it
plays; a change to any of the others starts it again from the sentence it
was at, with the new setting (paused, if it was), once the pointer is let go.

Blend with mixes a second voice into the first, and Mix says how much of it,
in tenths: Heart 70% · George 30% is sent as `af_heart(7)+bm_george(3)`. It
reads in the first voice's language.

Save as audio saves the file's speech, with the settings chosen now, in the
format picked (MP3, Opus, Ogg Vorbis, FLAC or WAV), and with Subtitles on an
`.srt` beside it. The desktop's save dialog says where; a name typed without
the suffix gets it. The worker saves beside any reading, so Read aloud keeps
playing, and the player shows how far the save is, with Cancel. A cancelled
save leaves no file.
The worker loads the model only for the first translated reading. Until the
first audio plays, the player shows a spinner with Starting… (or
Translating…), since loading the models and translating the first sentence
take seconds.

The bar beside the volume shows how far through the text the reading is.
Click or drag it, or focus it and press the arrow keys (5% a press), and the
reading starts again from the start of that sentence, with the settings
chosen now; a translated reading translates again from there. Each jump
pays the time to the first audio again.

Space pauses the reading where it is and resumes it (unless the search box
is being typed in). The window is also a media player on the session bus
(MPRIS, `org.mpris.MediaPlayer2.vinospeak`), so the desktop's media controls
show the file being read, its voice, the app's icon and a position, and
Play, Pause, Stop and the position slider there or on the keyboard's media
keys drive it; Play with nothing under way reads the file shown. The length
and position are estimates from the text (docs/architecture.md, "Media
controls").

About also shows the downloaded model's size, with Delete… to remove it
(`vinospeak --delete-model`). It waits for a reading or save to stop, and
stops the `vinospeak --serve` worker first; the next reading downloads the
model again.

About's Pronunciations edits `~/.config/vinospeak/lexicon.toml` (docs/usage.md,
"Pronunciations"): a word and how to say it, a row each. Save pronunciations
rewrites the file as a `[words]` table, so comments in it are not kept, and
the worker reads it again for the next reading. A file vinospeak would
refuse is shown as an error and left alone.

About's Speech API switch runs `vinospeak --http 8880` (OpenAI's
`/v1/audio/speech` on 127.0.0.1) while the window is open, and says the URL
once it is listening. It loads its own copy of the model, so it costs memory
on top of the window's worker. A server that exits (the port taken, say)
turns the switch off and says why.

`cargo test` drives the window headless with
[egui_kittest](https://crates.io/crates/egui_kittest), finding widgets by their
AccessKit labels, against a fake vinoSearch and a fake vinospeak script.
