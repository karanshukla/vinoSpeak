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
   headings (four across, or in a 2×2 grid in a narrow window).

Persistent options are under **Settings** at the bottom of the sidebar above
**About** (or as a tab in a narrow window). See [Settings](#settings).

Behavior details:

- **Persistence:** Voice, blend, speed, volume, translation language, save
  format, subtitle export, and the Speech API toggle are saved across runs in
  `~/.config/vinospeak/gui.json` (or under `$XDG_CONFIG_HOME`). Mute state is
  not saved, so the window always opens unmuted.
- **Volume and live changes:** Volume applies only to vinoSpeak rather than
  the system output, and changes take effect during playback. Changing
  **Translate**, **Voice**, **Speed**, **Output**, **Blend with**, or **Mix**
  restarts playback from the current sentence using the new setting once you
  release the control (staying paused if playback was paused).
- **Save as audio:** Exports spoken audio for the open file using the current
  voice, speed, and translation settings, along with the format and subtitle
  options configured in **Settings**. Choose the destination in the system save
  dialog (the file extension is added automatically if omitted). Export runs
  alongside playback without interrupting **Read aloud**, showing a progress bar
  and a **Cancel** button while running. Canceling removes any partial output
  file.
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
| Voice | **Blend with** mixes a second voice into the selected voice, and **Mix** sets the ratio in 10% steps (for example, Heart 70% · George 30% is sent as `af_heart(7)+bm_george(3)`). Playback uses the primary voice's language, and the player displays `Heart + George`. |
| Playback | **Output** selects the audio output device. |
| Save as audio | **Format** (MP3, Opus, Ogg Vorbis, FLAC, or WAV) and **Subtitles** (writes a matching `.srt` file next to the audio) used by the player's **Save as** button. |
| Pronunciations | Edits `~/.config/vinospeak/lexicon.toml` ([docs/usage.md](../docs/usage.md#pronunciations)) with one word and pronunciation respelling per row. Saving rewrites the file as a `[words]` table (comments are not preserved) and reloads it in the worker for subsequent playback. Invalid files are reported as an error and left unmodified. |
| Translation | Downloads or deletes the optional local translation model (see [Translation](#translation)). |
| Speech API | Starts `vinospeak --http 8880` (OpenAI-compatible `/v1/audio/speech` on `127.0.0.1`) while the window is open and displays the base URL once ready. Because the HTTP server loads its own model instance, it uses additional memory alongside the GUI worker. If the server exits (for example, if port 8880 is already in use), the switch turns off and displays the error. |
| Model | Shows the path to `vinospeak`, the cached Kokoro model size, and a **Delete…** button (see [Model management](#model-management)). |

The **About** page shows application details, update controls, and links to other
vinoPilot apps.

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
- The worker loads the translation model only on the first translated reading
  and unloads it after 5 minutes of inactivity. Until the first audio plays, the
  player displays a spinner labeled `Starting…` or `Translating…`, since loading
  models and translating the first sentence take several seconds.

## Model management

The **Settings** page displays the total size of the cached Kokoro model
(including OpenVINO's compiled cache) and provides a **Delete…** button
(`vinospeak --delete-model`). Deleting waits for any active reading to stop and
shuts down the `vinospeak --serve` worker first; the next reading re-downloads
the model.

## Headless testing and automation

Environment variables allow driving the window without interactive input:

- `VINOSPEAK_GUI_QUERY`: runs a search query on startup.
- `VINOSPEAK_GUI_SCREENSHOT=out.png`: saves a screenshot once the first hit's
  text loads, then exits.
- `VINOSPEAK_GUI_SPEAK=1`: starts reading before capturing the screenshot
  (values greater than 1 read for that many seconds instead of the default 4 s).
- `VINOSPEAK_GUI_SETTINGS=1`: opens the Settings page;
  `VINOSPEAK_GUI_SETTINGS=delete` opens it at the model delete confirmation.
- `VINOSPEAK_GUI_ABOUT=1`: opens the About page.

`cargo test` runs headless UI tests using
[egui_kittest](https://crates.io/crates/egui_kittest), locating widgets by their
AccessKit labels against a fake vinoSearch service and a fake `vinospeak`
script.
