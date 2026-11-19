# vinospeak-gui (proof of concept)

Desktop window for finding files with
[vinoSearch](https://github.com/karanshukla/vinoSearch), previewing the text
vinoSpeak will read, and listening to it. The GUI queries vinoSearch over D-Bus
(via its GNOME search provider, which works on any desktop) and delegates all
speech synthesis to the `vinospeak` CLI so the window process never loads a
model itself.

## Running and installing

```bash
cargo run --release                                    # uses vinospeak from PATH or ~/.local/bin
VINOSPEAK=../.venv/bin/vinospeak cargo run --release   # uses a checkout's vinospeak
VINOSPEAK=../.venv/bin/vinospeak ./install.sh          # installs into the app menu (--uninstall removes it)
```

On Fedora Atomic or openSUSE Aeon, you do not need Cargo: pass `--gui` to the
top-level installer to fetch the prebuilt release binary:

```bash
curl -fsSL https://github.com/karanshukla/vinoSpeak/releases/latest/download/install.sh | bash -s -- --gui
```

If you want to build from a checkout on an image-based distro, run `install.sh`
inside a toolbox or distrobox that has Cargo. The binary installs into
`~/.cargo/bin` and the menu entry into `~/.local/share`, both of which are
visible on the host. Use a container matching the host's OS release (the default
for `toolbox create`, or a Tumbleweed image on openSUSE Aeon) so the binary does
not link against a newer glibc than the host provides.

## File sources and reading

[vinoSearch](https://github.com/karanshukla/vinoSearch) is optional:

- **Without vinoSearch:** Search is disabled, and the window lists vinoWhisper
  sessions from both its native directory (`~/.local/share/vinowhisper/transcripts`)
  and its Flatpak directory (`~/.var/app/io.github.karanshukla.vinowhisper/data/...`).
  You can still open any text file via the folder button, drag-and-drop, or
  `vinospeak-gui FILE`.
- **With vinoSearch:** vinoWhisper sessions appear alongside recent files from
  vinoSearch.

Plain text and Markdown files are read directly by `vinospeak`, while PDFs and
other documents use the text extracted by vinoSearch. Each vinoWhisper session
is titled by its summary (or recording time) and offers a **Summary / Transcript**
toggle; **Summary** stays disabled until vinoWhisper has generated one.
Spreadsheets and source code are excluded from the GUI file lists even when
indexed by vinoSearch, though `vinospeak --file` can still read them from the
CLI.

## Player controls and settings

The player bar has two fixed rows that do not reflow:

1. **Transport:** Read aloud / Stop button, current status, progress bar, and
   volume control.
2. **Per reading:** Translate, Voice, Speed, and Save as audio under labeled
   headings (four across, or two by two in a narrow window).

What is set once rather than per reading is on **Settings**, at the foot of
the sidebar above About (a tab in a narrow window): see [Settings](#settings).

Behavior details:

- **Persistence:** Voice and blend, speed, volume, output, Translate, the save
  format and Subtitles, and whether the speech API is on are saved across runs
  in `~/.config/vinospeak/gui.json` (or under `$XDG_CONFIG_HOME`). Mute is not
  saved, so the window always opens unmuted.
- **Volume and live changes:** Volume applies only to the reading rather than
  the system output, and changes take effect mid-reading. Changing Translate,
  Voice, Speed, Output, Blend with or Mix restarts playback from the current
  sentence with the new setting (staying paused if it was paused), once the
  pointer is let go.
- **Save as audio:** saves the file's speech with the current settings, in the
  format and with the subtitles chosen on Settings. The desktop's save dialog
  says where; a name typed without the suffix gets it. The worker saves beside
  any reading, so Read aloud keeps playing; while it saves, the cell shows its
  progress and Cancel. A cancelled save leaves no file.
- **Seeking:** Click or drag the progress bar beside the volume control, or
  focus it and use the arrow keys (5% per press), to restart playback from the
  start of that sentence with the current settings. Translated readings
  re-translate from the new position. Each jump incurs the initial time-to-audio
  latency again.
- **Keyboard and MPRIS:** Pressing Space pauses and resumes playback (unless the
  search box has focus). The window also exposes an MPRIS player on the session
  bus (`org.mpris.MediaPlayer2.vinospeak`), so desktop media widgets and media
  keys display the current file, voice, app icon, and position, and can drive
  Play, Pause, Stop, and seeking. Pressing Play while idle reads the currently
  displayed file. Track length and position are estimated from the text (see
  [docs/architecture.md](../docs/architecture.md#media-controls)).

## Settings

| Group | What it does |
|---|---|
| Voice | **Blend with** mixes a second voice into the player's Voice, and **Mix** says how much, in tenths: Heart 70% · George 30% is sent as `af_heart(7)+bm_george(3)`. It reads in the first voice's language, and the player's Voice shows `Heart + George`. |
| Playback | **Output**: where speech plays. |
| Save as audio | **Format** (MP3, Opus, Ogg Vorbis, FLAC or WAV) and **Subtitles** (an `.srt` beside the audio) for the player's Save as. |
| Pronunciations | Edits `~/.config/vinospeak/lexicon.toml` ([docs/usage.md](../docs/usage.md#pronunciations)), a word and how to say it per row. Saving rewrites the file as a `[words]` table, so comments in it are not kept, and the worker reads it again for the next reading. A file vinospeak would refuse is shown as an error and left alone. |
| Translation | Downloads or deletes the translation model (below). |
| Speech API | A switch that runs `vinospeak --http 8880` (OpenAI's `/v1/audio/speech` on 127.0.0.1) while the window is open, and shows the URL once it is listening. It loads its own copy of the model, so it costs memory on top of the window's worker. A server that exits (the port taken, say) turns the switch off and says why. |
| Model | The Kokoro model's size, and Delete (below). |

About shows the app, its updates and the other vinoPilot apps.

## Translation

On the **Settings** page, the **Translation** row lets you confirm and download
SmolLM3-3B (1.7 GB, equivalent to `vinospeak --download-translation-model`) or
delete it later (unless vinoWhisper shares the same copy).

- Before the model is downloaded, clicking **Translate…** in the player opens
  that row on the Settings page.
- Once downloaded, **Translate** becomes a language picker. Selecting a language
  filters the voice picker to voices for that language, and clicking **Read
  aloud** translates and speaks the file incrementally.
- While reading, translated pieces appear as they arrive with the currently
  spoken piece highlighted. A **Translation / Original** toggle above the text
  switches back to the source text, and the translation stays attached to that
  file until the next reading.
- The worker loads the translation model only on the first translated reading.
  Until the first audio plays, the player displays a spinner labeled
  `Starting…` or `Translating…`, since loading models and translating the first
  sentence take several seconds.

## Model management

The **Settings** page displays the total size of the cached Kokoro model and
provides a **Delete…** button (`vinospeak --delete-model`). Deleting waits for
any active reading to stop and shuts down the `vinospeak --serve` worker first;
the next reading re-downloads the model.

## Headless testing and automation

Environment variables allow driving the window without interactive input:

- `VINOSPEAK_GUI_QUERY` — runs a search query on startup.
- `VINOSPEAK_GUI_SCREENSHOT=out.png` — saves a screenshot once the first hit's
  text loads, then exits.
- `VINOSPEAK_GUI_SPEAK=1` — starts reading before capturing the screenshot
  (values greater than 1 read for that many seconds instead of the default 4 s).
- `VINOSPEAK_GUI_SETTINGS=1` — opens the Settings page;
  `VINOSPEAK_GUI_SETTINGS=delete` opens it at the model delete confirmation.
- `VINOSPEAK_GUI_ABOUT=1` — opens the About page.

`cargo test` runs headless UI tests using
[egui_kittest](https://crates.io/crates/egui_kittest), locating widgets by their
AccessKit labels against a fake vinoSearch service and a fake `vinospeak`
script.
