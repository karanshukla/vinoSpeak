# Architecture

Why vinoSpeak is built the way it is, with the measurements behind each
choice. Every number here is n=1 on the machine named beside it.

## Layout

| Module | Job |
|---|---|
| `vino_speak/tts.py` | `Synthesizer` (the loaded pipeline) and the one-shot `synthesize()` / `speak()` |
| `vino_speak/text.py` | `chunks()`: the text cut into pieces synthesized one at a time |
| `vino_speak/playback.py` | `Speech`: raw samples into one player process, with `stop()` |
| `vino_speak/model.py` | The cache directory, the pinned download and its verification |
| `vino_speak/readable.py` | A file as text worth hearing: markdown, extracted documents, plain text |
| `vino_speak/session.py` | A vinoWhisper `.vwsession` read as its summary or transcript |
| `vino_speak/serve.py` | `vinospeak --serve`, the window's worker |
| `vino_speak/cli.py`, `doctor.py` | The `vinospeak` and `vinospeak-doctor` commands |
| `vino_speak/ovfetch.py` | `ovfetch detect` read into notes, and the NPU compiler preload |
| `gui/` | The window (Rust, egui); see [gui/README.md](../gui/README.md) |

`import vino_speak` imports no OpenVINO, numpy or huggingface_hub. The heavy
imports happen inside `Synthesizer.__init__` and `ensure_model()`.

## Telemetry

OpenVINO bundles `openvino_telemetry`, loaded through `ovc` when `openvino` is
imported. It takes a missing `~/intel/openvino_telemetry` consent file as
consent and reports usage to Google Analytics.

`vino_speak/__init__.py` puts a `None` entry in `sys.modules` for it, which
makes it unimportable in this process, so `ovc` falls back to its no-op stub.
The entry has to be there before `openvino` is imported, which is why it sits
at the top of the package: every entry point imports the package first. The
consent file is left alone, since it is the user's choice for their other
OpenVINO tools. `tests/test_telemetry.py` pins both halves.

## The NPU vocoder override

OpenVINO 2026.4 pins Kokoro's vocoder to the CPU, for an NPU compiler bug
that driver 1.32.0 on Wildcat Lake doesn't have (2026-10-17). vinoSpeak
passes `NPUW_SUBMODEL_DEVICE=""` on the NPU to undo the pin, so the whole
model runs there.

Other drivers may have the bug, so the override can fail in two places, and
both fall back to OpenVINO's pin with a `RuntimeWarning`:

- The pipeline won't compile with the override: it is built again without it.
- The first `generate()` raises or returns non-finite samples: until one
  `generate()` has worked with the override, a failure may be the compiler bug
  it exposes, so it gets one retry with the pin. After one success, errors are
  raised as they are.

Speech still works on the pin but saves no battery (vinoSpeak#12). The
override goes away with vinoSpeak#7. The tests are
`test_an_npu_that_cant_compile_the_vocoder_falls_back_to_the_pin`,
`test_a_first_generate_that_fails_on_the_npu_retries_with_the_pin`,
`test_non_finite_audio_on_the_npu_retries_with_the_pin` and
`test_once_the_override_has_worked_errors_are_not_retried` in
`tests/test_tts.py`.

## The NPU compiler preload

Fedora's `intel-npu-driver` 1.32.0 rpm ships no NPU compiler. `ovfetch npu
install` (ovfetch 0.6.0) puts Intel's in `~/.local/lib/ovfetch-npu`, and
`ovfetch.preload()` loads each line of that prefix's `ovfetch-preload.txt`
with `RTLD_GLOBAL` before the first pipeline or `Core()`.

The order of the list is the only safe one: a driver loaded before the Level
Zero loader crashed (ovfetch's README, 2026-10-24). A prefix that won't load
is fatal only when the device asked for is the NPU. `ovfetch.py` is the same
code as vinoWhisper's on purpose.

## ovfetch

ovfetch's `detect` reports the OpenVINO range Intel records as working with
this machine's NPU driver. Like vinoWhisper and vinoPop, vinoSpeak only
reports that range. The bounds are pessimistic on purpose ("untested", not
"known to fail"), so `vinospeak-doctor` shows them as warnings and nothing
stops the NPU from being tried.

vinoSpeak runs OpenVINO GenAI from pip (pinned in `pyproject.toml`), not ONNX
Runtime, so it never runs `ovfetch install`. The one thing pip can't provide
is the NPU compiler above.

## The compile cache

Every pipeline is given `CACHE_DIR` (`ov-cache/` beside the model). Without it
each process compiles the model again: 6.5 s on CPU in the VM, 1.35 s from the
cache (2026-10-29).

## The worker's warm-up

`vinospeak --serve` synthesizes "Ready." once after loading and throws it
away. The first `generate()` costs about 2.5 s more than a warm one (VM, CPU,
2026-10-29), and this way the first reading doesn't pay it.

## Chunking

`chunks()` puts the first sentence alone (or its first clause, past 60
characters, if the sentence is long), then chunks that at most double in
length, up to 400 characters.

Synthesis runs at roughly a third of real time (2026-10-17, NPU), so each
chunk is ready before the one before it has finished playing, and the rest of
the text goes in as few calls as that allows. Every call pays a fixed cost,
which is why no chunk after the first is cut under 100 characters.

A chunk is never cut after an abbreviation or an initial (`e.g.`, `Dr.`,
`J. R.`): Kokoro's pitch falls at the end of a chunk, and it would fall
mid-sentence.

## Voices and blends

A voice is a `(510, 1, 256)` float32 embedding, one style vector per input
length, and Kokoro mixes voices by taking their weighted mean, the same rule
Kokoro-FastAPI and kokoro's own `KPipeline` use. A blend is mixed once and
kept with the loaded voices under one spelling (`voices.canonical`), so
`af_heart(2)+bm_george(2)` and `af_heart+bm_george` share an embedding.
Checked on the real pipeline on CPU, 2026-11-18.

OpenVINO GenAI 2026.4's Kokoro refuses `ja` and `zh`, so the nine Japanese
and Mandarin voices in the model aren't offered, as in the window's picker.

## SSML and the lexicon

Everything spoken goes through `script.plan()`: SSML is parsed into pieces
with their own voice, language and rate, the lexicon respells each piece's
text (never the markup), and the pieces are cut into chunks, or into
sentences for subtitles. A pause is a step of silence that never reaches the
pipeline. Plans are made before the model loads, so a bad `<break>` or lexicon
fails at once.

Kokoro's pipeline in OpenVINO GenAI 2026.4 takes no phonemes: misaki's
`[word](/IPA/)` is ignored (a word given 18 `ʃ` phonemes came out the length
of the word, CPU, 2026-11-18), which is why the lexicon respells.

## Playback

Players are tried in the order `pw-play`, `paplay`, `aplay`: PipeWire is the
sound server Wayland desktops run, and PulseAudio and bare ALSA cover the
rest. `pw-play` only learned `--raw` after PipeWire 1.0 (Ubuntu 24.04 ships
1.0.5, Debian 12 ships 0.3.65), so it is asked for its `--help` and skipped
when `--raw` isn't in it.

`Speech` runs two threads, one synthesizing and one feeding the player, with
a queue of one chunk between them. One chunk waiting is enough to keep
playback fed without spending energy far ahead of a `stop()`
(`test_synthesis_runs_one_chunk_ahead_of_a_player_that_is_not_reading`).

The window's volume is the reading's own, not the output's: `Speech` scales
the samples as it writes them, so it works the same through all three
players and never moves the desktop's volume for other apps. A chunk is
written a tenth of a second at a time, so a change made mid-reading is heard
after that slice plus what the pipe and player already hold (under a second
with the default 64 KiB pipe at 24 kHz float32).

Two failure paths that are easy to lose:

- A player that won't start (gone, not executable, or one `raw_command` can't
  drive) would end the playback thread silently, and `wait()` would report
  speech that never played. The error is kept and raised from `wait()`.
- When playback ends for any reason, the synthesis thread is released. It
  would otherwise wait on a full queue for a `stop()` that a library caller
  may never send.

## The model on disk

The download is pinned to one Hugging Face commit, and every file is checked
against the sha256 in `model_pins.json`. [SECURITY.md](../SECURITY.md) says
what that does and doesn't protect against.

- A stamp file (`.vinospeak-verified`) is written once every file has matched.
  A stamped model is checked by size on each run. Without the stamp, or with
  one from older pins, the whole model is hashed first, so a download that
  failed verification is never used just because its files have the right
  sizes.
- A lock file is held shared while the default model downloads and
  exclusively while it is deleted, so a delete can't pull the files out from
  under a download.
- A `--model-dir` that already holds the model files is used as given and
  never deleted.

## Shared with the window

The window (Rust) and `vinospeak` (Python) both look at the model cache, and
they can't share code. The rule both follow:

- The cache is `$XDG_CACHE_HOME/vinospeak`, or `~/.cache/vinospeak` when that
  variable is unset or empty. The model is `kokoro-82m-int8-ov/` in it, with
  its voices in `voices/*.bin`, and OpenVINO's compiled copy is `ov-cache/`.
- A size on disk is the bytes of the regular files at any depth. A symlink is
  not followed and not counted.
- A size is shown in decimal units: one decimal from 1 GB up, whole megabytes
  from 1 MB up, whole kilobytes under that (`163 MB`, `1.3 GB`, `1 kB`).
- A fenced code block is read as "Code skipped." (`readable.CODE_SKIPPED`),
  which the window's reader sets apart from the text around it.

`tests/fixtures/shared_with_window.json` holds the paths, the sizes and that
sentence, and both sides are tested against it:
`test_the_cache_is_where_the_window_looks_for_it`,
`test_sizes_are_shown_as_the_window_shows_them` and
`test_a_skipped_code_block_is_said_as_the_window_expects` in Python,
`the_cache_is_where_vinospeak_keeps_it` and
`sizes_are_shown_as_vinospeak_shows_them` in `gui/src/model_cache.rs`, and
`the_code_stand_in_is_what_vinospeak_prints_for_a_code_block` in
`gui/src/app/reader.rs`. The size table has a pair either side of each unit's
boundary.

The two differ in what they add up. `vinospeak --model-status` lists the
model and the compiled copy separately; the window's About page shows one
figure for everything under the cache directory. An exact
half rounds to even on both sides (2.5 MB is `2 MB`, 3.5 MB is `4 MB`), and
the table has those rows; until 2026-11-15 the window rounded a half up.

The window walks the cache with `walkdir`, which doesn't follow links unless
asked. It was already in `Cargo.lock` for another platform's build, so the
lock gains no package;
`disk_usage_counts_neither_a_link_nor_what_it_points_to` pins that.

The window formats a size with `humansize` (2026-11-15), told which unit and
how many decimals by the three rules above. Python's `cli.size` stays written
out: `humanize.naturalsize` can't give these strings (it says `999 Bytes`
where the rule is `1 kB`, and one format for every unit where GB alone has a
decimal), and it would be a runtime dependency of a package whose import has
to stay light.

## Reading markdown

The inline patterns in `readable.py` stop at the next opening delimiter, so
an unclosed `[` or `**` costs a scan to the next one instead of to the end of
the line. With `.+?`, a line of 5,000 unclosed `**a ` took 3.3 s, growing
with the square of its length, and a large `.md` file could hang the window's
preview (2026-10-25). 25,000 now take about 20 ms.
`test_unclosed_inline_markup_is_linear` pins it.

In an extracted document, a word broken over a line end is rejoined only when
both halves are lowercase: `infor-` / `mation` loses its hyphen, `COVID-` /
`19` and `Jean-` / `Paul` keep theirs, since only lowercase halves can be
typesetting.

## vinoWhisper sessions

A `.vwsession` is read with `zipfile`, as vinoSearch reads it, so vinoWhisper
needn't be installed. The format is vinoWhisper's `docs/sessions.md`. A bundle
can come from anywhere a file manager opens one, so the sizes in its header
aren't trusted: reads stop at vinoWhisper's own limits (64 MiB a part, 128 MiB
in all, 16 entries) whatever the header claims.

`session.load()` checks in this order: the `mimetype` entry is first, stored
and correct; the entry count; the total of the three parts it reads; then each
part as it is read. `tests/test_session.py` has a pair either side of each
limit.

The transcript's "Recorded on 27 October 2026" uses a month list written out
in `session.py`. `calendar.month_name` would follow the process's locale, and
a program that embeds vinoSpeak (vinoPop's window) sets one; a German month
name read by an English voice is worse than twelve literals.
