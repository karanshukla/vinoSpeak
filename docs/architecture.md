# Architecture

Design decisions for vinoSpeak along with the measurements behind each choice.
All measurements below are `n=1` on the machine noted beside them.

## Layout

| Module | Responsibility |
|---|---|
| `vino_speak/tts.py` | `Synthesizer` (loaded pipeline) and one-shot `synthesize()` / `speak()` helpers |
| `vino_speak/text.py` | `chunks()` for splitting text into pieces synthesized one at a time |
| `vino_speak/playback.py` | `Speech`: streams raw audio samples into a player process with `stop()` support |
| `vino_speak/model.py` | Cache directory layout, pinned model downloads, and hash verification |
| `vino_speak/readable.py` | Converts Markdown, extracted documents, and plain text into speakable text |
| `vino_speak/session.py` | Reads vinoWhisper `.vwsession` bundles as summary or transcript |
| `vino_speak/serve.py` | `vinospeak --serve`, the background worker for the desktop window |
| `vino_speak/cli.py`, `doctor.py` | The `vinospeak` and `vinospeak-doctor` CLI commands |
| `vino_speak/ovfetch.py` | Parses `ovfetch detect` output into diagnostic notes and preloads the NPU compiler |
| `gui/` | Desktop window (Rust, egui); see [gui/README.md](../gui/README.md) |

Importing `vino_speak` does not import OpenVINO, NumPy, or `huggingface_hub`.
Those heavier imports happen inside `Synthesizer.__init__` and `ensure_model()`.

## Telemetry

OpenVINO bundles `openvino_telemetry`, loaded through `ovc` when `openvino` is
imported. If `~/intel/openvino_telemetry` does not exist, OpenVINO treats its
absence as opt-in consent and reports usage to Google Analytics.

`vino_speak/__init__.py` inserts `None` into `sys.modules` for
`openvino_telemetry`, making it unimportable in the current process so `ovc`
falls back to its no-op stub. Because this must happen before `openvino` is
imported, it runs at the top of the package (every entry point imports the
package first). The user's consent file is left untouched for other OpenVINO
tools. `tests/test_telemetry.py` verifies both behaviors.

## The NPU vocoder override

OpenVINO 2026.4 pins Kokoro's vocoder to the CPU to work around an NPU compiler
bug that driver 1.32.0 on Wildcat Lake does not exhibit (verified 2026-10-17).
vinoSpeak passes `NPUW_SUBMODEL_DEVICE=""` on the NPU to clear that pin so the
entire model runs on the NPU.

Because other drivers may still hit the bug, vinoSpeak handles two failure paths
by falling back to OpenVINO's default pin with a `RuntimeWarning`:

- **Compilation failure:** If the pipeline fails to compile with the override,
  vinoSpeak rebuilds it without the override.
- **First generation failure:** If the first `generate()` call raises or returns
  non-finite samples, vinoSpeak retries once with the pin. Once a single
  `generate()` call succeeds with the override, subsequent errors propagate
  normally.

Speech still works with the CPU pin, though without the NPU battery savings
(vinoSpeak#12). This override will be removed in vinoSpeak#7. Pinned by
`test_an_npu_that_cant_compile_the_vocoder_falls_back_to_the_pin`,
`test_a_first_generate_that_fails_on_the_npu_retries_with_the_pin`,
`test_non_finite_audio_on_the_npu_retries_with_the_pin`, and
`test_once_the_override_has_worked_errors_are_not_retried` in
`tests/test_tts.py`.

## The NPU compiler preload

Fedora's `intel-npu-driver` 1.32.0 RPM does not include an NPU compiler.
`ovfetch npu install` (ovfetch 0.6.0) installs Intel's compiler into
`~/.local/lib/ovfetch-npu`, and `ovfetch.preload()` loads each library listed in
that prefix's `ovfetch-preload.txt` using `RTLD_GLOBAL` before the first
pipeline or `Core()` is constructed.

The load order in `ovfetch-preload.txt` is required: loading the driver before
the Level Zero loader causes a crash (documented in ovfetch's README,
2026-10-24). A prefix that fails to load is fatal only when targeting the NPU.
`ovfetch.py` is intentionally identical to vinoWhisper's copy.

## ovfetch

`ovfetch detect` reports the OpenVINO version range Intel records as tested
with the machine's NPU driver. Like vinoWhisper and vinoPop, vinoSpeak treats
this as advisory information only: the bounds are conservative ("untested"
rather than "known to fail"), so `vinospeak-doctor` surfaces mismatches as
warnings without blocking NPU execution.

vinoSpeak installs OpenVINO GenAI from pip (pinned in `pyproject.toml`) rather
than ONNX Runtime, so it never invokes `ovfetch install`. The only component
pip cannot provide is the NPU compiler described above.

## The compile cache

Every pipeline sets `CACHE_DIR` to `ov-cache/` alongside the model files.
Without it, each process recompiles the model from scratch: 6.5 s cold versus
1.35 s cached on CPU in the VM (measured 2026-10-29).

## The worker's warm-up

`vinospeak --serve` synthesizes `"Ready."` once after loading and discards the
audio. The first `generate()` call takes about 2.5 s longer than subsequent warm
calls (VM, CPU, 2026-10-29), so warming up at startup keeps that latency off
the user's first reading.

## Chunking

`chunks()` isolates the first sentence (or its first clause past 60 characters
if the sentence is long), then produces chunks that at most double in length up
to 400 characters.

Because NPU synthesis runs at roughly one-third of real time (measured
2026-10-17, NPU), each chunk finishes before the previous one finishes playing,
while growing the chunk size minimizes total pipeline calls. Every call carries
fixed overhead, so no chunk after the first is split below 100 characters.

Chunks are never split after abbreviations or initials (`e.g.`, `Dr.`, `J. R.`),
because Kokoro drops pitch at chunk boundaries and splitting there would sound
like a mid-sentence full stop.

### Symbols at the end

Outside English, Kokoro returns an empty waveform when a text ends with certain
punctuation or symbol characters, aborting playback with `"The generated waveform
is empty or contains non-finite values."`.

Measured on the NPU (2026-11-18) using `"Hola amigos."` plus a trailing symbol
across Spanish, French, Italian, and Brazilian Portuguese:

- **Failed in all four languages:** `< > | _ ` " ' « » – — …` and curly quotes.
- **Failed in some languages:** `/ \ # ^ ~ =`.
- **English:** Read all of them without error.
- **Mid-text or leading position:** Succeeded in all tested languages.

Because clause splits after `—` or `–` can leave a trailing dash and SmolLM3
sometimes wraps responses in `<…>`, readings can hit this in practice.
`speakable_end()` strips trailing runs of these characters before synthesis for
non-English text, while keeping `%`, `&`, `+`, `)`, and similar symbols that
Kokoro pronounces normally.

## Voices and blends

Each voice is a `(510, 1, 256)` `float32` embedding (one style vector per input
length). Voice blends take the weighted mean of their embeddings, matching
Kokoro-FastAPI and Kokoro's own `KPipeline`.

A blend is computed once and cached with the loaded voices under a canonical
key (`voices.canonical`), so `af_heart(2)+bm_george(2)` and `af_heart+bm_george`
share one embedding (verified against the real pipeline on CPU, 2026-11-18).

OpenVINO GenAI 2026.4's Kokoro pipeline rejects `ja` and `zh`, so the nine
Japanese and Mandarin voices present in the model are omitted from both the CLI
and the GUI voice picker.

## SSML and the lexicon

All spoken input passes through `script.plan()`:

1. SSML is parsed into segments carrying their own voice, language, and rate.
2. The pronunciation lexicon respells each segment's text (never markup).
3. Segments are split into synthesis chunks (or individual sentences when
   generating subtitles).

Pauses (`<break>`) become silence steps that never hit the synthesis pipeline.
Planning runs before model loading so invalid `<break>` values or malformed
lexicons fail immediately.

OpenVINO GenAI 2026.4's Kokoro pipeline ignores inline phonemes (misaki's
`[word](/IPA/)` syntax had no effect when tested with 18 `ʃ` phonemes, producing
audio the length of the plain word on CPU, 2026-11-18), which is why the lexicon
uses plain-text respellings.

## Translation

`--translate` runs SmolLM3-3B (vinoWhisper's INT4 channel-wise export, with pins
copied into `translation_pins.json`) through OpenVINO GenAI's `LLMPipeline`
using greedy decoding and `/no_think`, matching vinoWhisper.

vinoSpeak loads its own instance rather than talking to vinoWhisper's server so
each vinoPilot app stays standalone. Because running both models at once adds
~2 GB of memory, the translation model loads lazily on the first translation and
unloads after `IDLE_UNLOAD_S` (five minutes) of inactivity.

On the NPU, the pipeline uses vinoWhisper's context window (`MAX_PROMPT_LEN`
3072, `MIN_RESPONSE_LEN` 1024), capping input pieces at 1,500 characters and
outputs at 1,024 tokens. Pieces follow `chunks()` (the first sentence alone,
then pieces that at most double in length) so playback starts after translating
one sentence rather than the whole file. One piece is translated ahead on a
background thread while the previous piece is synthesized and played.

### Storage and sharing with vinoWhisper

- **Shared directory:** The model is stored in
  `$XDG_DATA_HOME/vinowhisper/models/SmolLM3-3B-int4-cw-ov/` (same sharing rule
  as vinoPop's Gemma), so installing both apps keeps a single 1.7 GB copy.
- **Shared pins and markers:** Both apps share pins; vinoSpeak accepts
  vinoWhisper's `.vinowhisper-revision` at the pinned revision and writes one
  after its own verified download.
- **Deletion:** Deleting the translation model leaves any copy vinoWhisper has
  used (marked by `.vinowhisper-verified`) intact and removes only vinoSpeak's
  compiled copy in `$XDG_DATA_HOME/vinospeak/translation/`. Keeping both outside
  `$XDG_CACHE_HOME` prevents a cache purge from wiping a 1.7 GB opt-in download
  and keeps the GUI's Kokoro disk-usage figure scoped to the cache directory.
- **Flatpak fallback:** If no native copy exists, vinoSpeak can read from
  vinoWhisper's Flatpak data directory
  (`~/.var/app/io.github.karanshukla.vinowhisper/data/`), without downloading
  into or deleting from that sandboxed folder. Because the sandbox can write to
  that directory while vinoSpeak loads it outside the sandbox, every file is
  hashed against the pins before first use (taking a few seconds for 1.7 GB) and
  cached in vinoSpeak's own `flatpak-verified` record with each file's size,
  mtime, ctime, and inode. Since the sandbox cannot fake `ctime`, any
  modification forces re-hashing. The GUI shows the **Translate** control based
  on vinoWhisper's marker alone; if verification fails at read time, the reading
  reports the model error.
- **Discoverability:** Before the model is downloaded, the GUI displays a
  **Translate…** button that jumps to the Settings page's download confirmation;
  when it was hidden beforehand, users could not find it (2026-11-18).

### Prompting and model limits

The translation instruction is placed in both the system prompt and the user
turn, with the source text wrapped in `<text>` tags.

Measured on the Wildcat Lake NPU (2026-11-18):

- With bare text and only a system prompt (which also instructed the model to
  repeat text already in the target language), Spanish returned English
  untranslated for `"Hello there."`, `"Good morning."`, and a full English
  sentence, and turned `"The weather is lovely."` into `"What's the weather
  like?"`.
- With the `<text>` request form, all six test lines translated correctly into
  Spanish and Italian.
- Into British English, the model occasionally closed `</text>` early and
  appended a note, so `tidy()` truncates at the first `</text>`.
- Brazilian Portuguese still produced factual translation errors on the same
  lines (`"Good morning"` -> `"Boa tarde"`, Thursday -> `quarta-feira`), which
  is a limitation of the 3B model rather than the prompt.

End-to-end streaming and worker events were also verified in `linuxlab` using an
LLM stand-in (`claude -p`, 2026-11-19); that validates chunking, streaming, and
worker events, while the prompt tests above ran on the real SmolLM3 model on the
NPU (not NPU memory usage with both models loaded simultaneously).

## Playback

Audio players are probed in order: `pw-play`, `paplay`, then `aplay`. PipeWire
covers modern Wayland desktops while PulseAudio and ALSA cover other setups.
Because `pw-play` only gained `--raw` after PipeWire 1.0 (Ubuntu 24.04 ships
1.0.5, Debian 12 ships 0.3.65), vinoSpeak checks `pw-play --help` and skips it
if `--raw` is absent.

`Speech` uses two threads (one synthesizing and one feeding the player)
connected by a single-chunk queue. Buffering one chunk ahead keeps audio
continuous while avoiding wasted synthesis if `stop()` is called
(`test_synthesis_runs_one_chunk_ahead_of_a_player_that_is_not_reading`).

### Progress and volume

Progress is tracked by text share rather than audio duration, because total
audio length is unknown until the final chunk is synthesized:

- Each chunk carries its fraction of the total reading text, and the playback
  thread advances through that fraction as 0.1 s slices are written.
- Translated chunks inherit their source piece's share so progress moves
  smoothly even when translation length differs from the source.
- The worker emits a `progress` event every 0.5%, and the GUI progress bar never
  moves backward.
- Each `translated` event carries the same span (`translate.spans`), letting the
  GUI highlight whichever piece overlaps the current progress (approximate due
  to pipe buffering and variable speech rate).

Volume is applied inside `Speech` by scaling samples before writing them to the
player. This behaves consistently across all three players without altering
desktop volume for other apps. Because chunks are written in 0.1 s slices,
volume changes take effect within a fraction of a second (under 1 s with the
default 64 KiB pipe buffer at 24 kHz `float32`).

### Playback error handling

Two edge cases are handled explicitly:

- If a player fails to launch (missing, not executable, or unsupported by
  `raw_command`), the playback thread captures the exception and `wait()`
  re-raises it instead of reporting silent success.
- Whenever playback stops for any reason, the synthesis thread is unblocked so
  it does not hang on a full queue waiting for a `stop()` call that a library
  caller may never issue.

## The model on disk

Downloads are pinned to a specific Hugging Face commit, and every file is
verified against the SHA-256 digests in `model_pins.json` (see
[SECURITY.md](../SECURITY.md) for the security boundaries).

- **Verification stamp:** `.vinospeak-verified` is written only after all files
  match. Subsequent runs check file sizes; if the stamp is missing or from older
  pins, all files are re-hashed before use so a corrupted file with a matching
  size cannot slip through.
- **Locking:** A lock file is held shared during downloads and exclusively
  during deletion so a delete cannot remove files out from under an active
  download.
- **Custom model directories:** A `--model-dir` that already contains model
  files is used unchecked and never deleted.

## Shared with the window

The Rust GUI (`gui/`) and Python CLI (`vino_speak/`) cannot share code directly,
so both follow the same contract for the model cache and formatting:

- **Cache paths:** `$XDG_CACHE_HOME/vinospeak` (falling back to
  `~/.cache/vinospeak` when unset or empty), containing `kokoro-82m-int8-ov/`
  (with voices in `voices/*.bin`) and OpenVINO's `ov-cache/`.
- **Disk usage:** Sum of regular file byte sizes at any depth; symlinks are
  neither followed nor counted.
- **Size formatting:** Decimal units (one decimal place from 1 GB up, whole
  megabytes from 1 MB up, and whole kilobytes below that: `163 MB`, `1.3 GB`,
  `1 kB`), with exact halves rounded to even (`2.5 MB` -> `2 MB`, `3.5 MB` ->
  `4 MB`; prior to 2026-11-15 the window rounded halves up).
- **Skipped code marker:** Fenced code blocks are represented as
  `"Code skipped."` (`readable.CODE_SKIPPED`), which the GUI reader styles
  distinctly from surrounding text.

Both sides test against `tests/fixtures/shared_with_window.json` (which includes
boundary pairs around every unit threshold and half-to-even rounding rows):

- Python: `test_the_cache_is_where_the_window_looks_for_it`,
  `test_sizes_are_shown_as_the_window_shows_them`, and
  `test_a_skipped_code_block_is_said_as_the_window_expects`.
- Rust: `the_cache_is_where_vinospeak_keeps_it` and
  `sizes_are_shown_as_vinospeak_shows_them` in `gui/src/model_cache.rs`, and
  `the_code_stand_in_is_what_vinospeak_prints_for_a_code_block` in
  `gui/src/app/reader.rs`.

The one intentional difference is scope: `vinospeak --model-status` reports the
model and compiled cache separately, whereas the GUI's Settings page shows a single
total for the entire cache directory.

Implementation notes:

- The GUI traverses the cache with `walkdir` (which ignores symlinks by default
  and was already in `Cargo.lock` for another platform's build, adding no new
  dependency; pinned by
  `disk_usage_counts_neither_a_link_nor_what_it_points_to`).
- The GUI formats sizes with `humansize` (2026-11-15) configured to the three
  rules above. Python keeps a small `cli.size` helper rather than adding
  `humanize` as a runtime dependency, both to keep imports light and because
  `humanize.naturalsize` does not match these formatting rules (`999 Bytes`
  instead of `1 kB`, and uniform decimal precision across units).

## Media controls

The window registers an MPRIS player (`org.mpris.MediaPlayer2.vinospeak`) on the
D-Bus session bus (the bus desktops query for user media players, unlike the
system bus). On startup it writes the app icon into `$XDG_RUNTIME_DIR` for
`mpris:artUrl` so Plasma does not display a blank disc.

Because MPRIS tracks playback in time while vinoSpeak only knows text progress
until synthesis completes, the window estimates track length from character count
and Kokoro's speaking rate:

- Measured on the Wildcat Lake NPU (2026-11-19) over three English sentences
  (310 characters) at 1.0×: `af_heart` 15.44 chars/s, `ef_dora` 16.79 chars/s,
  `ff_siwis` 15.49 chars/s.
- The window uses `15.5` characters/s divided by the active speed, and estimates
  translated readings from the source text length.

Desktop shells extrapolate playback position while status is `Playing`, while
vinoSpeak updates progress in discrete steps. To avoid jitter, `Seeked` is only
emitted on a new reading or when the extrapolated and actual positions diverge
by more than 2 s. Desktop seek requests (`SetPosition`, `Seek`) map back to a
fraction of the estimated length and restart playback from that sentence,
matching the window's own progress bar.

## Reading markdown

Inline Markdown regexes in `readable.py` stop at the next opening delimiter so
an unclosed `[` or `**` scans only to the next opener rather than the end of the
line. Previously, using `.+?` caused quadratic runtime: a line with 5,000 unclosed
`**a ` tokens took 3.3 s and could hang the GUI preview (2026-10-25). With
bounded delimiters, 25,000 tokens take about 20 ms
(`test_unclosed_inline_markup_is_linear`).

In extracted documents (`--kind document`), hyphenated line breaks are rejoined
only when both halves are lowercase (`infor-` / `mation` -> `information`),
preserving intentional hyphens in `COVID-` / `19` and `Jean-` / `Paul`.

## vinoWhisper sessions

`.vwsession` bundles are read directly with Python's `zipfile` (the same way
vinoSearch reads them), so vinoWhisper does not need to be installed. The format
is documented in vinoWhisper's `docs/sessions.md`.

Because a bundle may come from an untrusted source, archive header sizes are not
trusted and reads enforce vinoWhisper's size limits (64 MiB per part, 128 MiB
total, 16 entries max). `session.load()` validates in order:

1. The `mimetype` entry is first, uncompressed (stored), and exact.
2. Total entry count is within the limit.
3. Combined size of the three read parts is within the limit.
4. Each part is bounded as it is read.

`tests/test_session.py` includes boundary tests on either side of each limit.

The transcript heading (`"Recorded on 27 October 2026"`) uses a hardcoded
English month list in `session.py` rather than `calendar.month_name`, because
host applications that embed vinoSpeak (such as vinoPop) may set a non-English
process locale, which would feed a localized month name into an English voice.
