<p align="center">
  <img src="https://raw.githubusercontent.com/karanshukla/vinoSpeak/main/docs/assets/vinospeak.svg" width="112" alt="The vinoSpeak mark: a white speaker with two green sound waves on the dark vino tile">
</p>

# vinoSpeak

[![PyPI](https://img.shields.io/pypi/v/vinospeak?logo=pypi&logoColor=white)](https://pypi.org/project/vinospeak/)
[![Python](https://img.shields.io/pypi/pyversions/vinospeak)](https://pypi.org/project/vinospeak/)
[![License](https://img.shields.io/badge/license-MIT-blue)](https://github.com/karanshukla/vinoSpeak/blob/main/LICENSE)
[![CI](https://github.com/karanshukla/vinoSpeak/actions/workflows/ci.yml/badge.svg)](https://github.com/karanshukla/vinoSpeak/actions/workflows/ci.yml)
[![Ruff](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json)](https://github.com/astral-sh/ruff)

Local text-to-speech on the Intel NPU, using Kokoro-82M INT8 through OpenVINO
GenAI. Type a sentence or point it at a file and it reads it aloud. Nothing
leaves your machine after the one-time ~160 MB model download, and OpenVINO's
own telemetry is blocked.

- **Speaks text, files and stdin.** Markdown is cleaned up for listening, and
  PDFs, Word and slides work through [vinoSearch](https://github.com/karanshukla/vinoSearch)'s extracted text.
- **Reads [vinoWhisper](https://github.com/karanshukla/vinoWhisper) sessions**:
  the summary, or the transcript.
- **Starts talking in about 1.5 s on the NPU**, however long the text: the first
  sentence plays while the rest is synthesized.
- **41 Kokoro voices** in US/UK English, Spanish, French, Hindi, Italian and
  Brazilian Portuguese, at 0.5x to 2x speed.
- **Plays or saves**: your speakers via PipeWire/PulseAudio/ALSA, or a WAV.
- **NPU, GPU or CPU.** CPU works with no driver at all.
- A [window](gui/) (proof of concept) to find a file with vinoSearch and listen
  to it, and a Python API.

<p align="center"><img src="docs/assets/gui.svg" width="800" alt="The vinoSpeak window. Typing &quot;how often is the lens cleaned&quot; finds lighthouse-log.pdf through vinoSearch; Read aloud starts speaking it and the status bar says so. Then a vinoWhisper session, Harbour safety meeting, is picked and its summary is read aloud."></p>

## Install

Python 3.11 to 3.14. With [uv](https://docs.astral.sh/uv/):

```bash
uv tool install --python 3.14 vinospeak
vinospeak "Hello."
```

`--python 3.14` because OpenVINO 2026.4.1 has no 3.15 wheels yet. OpenVINO is
pinned to the same release as vinoPop and vinoWhisper, so programs sharing one
NPU never run mismatched runtimes. For the exact versions in `uv.lock`, use a
checkout: `uv sync --locked && uv run vinospeak "Hello."`.

**Fedora Atomic / openSUSE Aeon**: nothing to layer for vinoSpeak itself. uv
brings its own Python and the wheels carry espeak-ng and libsndfile. Only the
NPU driver (for `--device NPU`) and an audio player, if you lack one, are
system packages.

**No player?** Install `pw-play` or `paplay`, or use `--output`:

| Distro | `pw-play` | `paplay` |
|---|---|---|
| Fedora | `pipewire-utils` | `pulseaudio-utils` |
| Ubuntu / Debian | `pipewire-bin` | `pulseaudio-utils` |
| Arch | `pipewire` | `libpulse` |
| openSUSE | `pipewire-tools` | `pulseaudio-utils` |

## Use

```bash
vinospeak "Hello from vinoSpeak."                  # play it
vinospeak "Hello." --output hello.wav              # save a WAV
vinospeak --file notes.md                          # read a file aloud
vinospeak --file notes.md --print-text             # see what it would say, no model load
some-command | vinospeak --file -                  # plain text from stdin
vinospeak --file 2026-10-27-140512.vwsession       # a vinoWhisper summary
vinospeak --file 2026-10-27-140512.vwsession --part transcript
vinospeak "Hola." --voice ef_dora --language es --speed 1.2 --device CPU
```

| Option | What it does |
|---|---|
| `--voice`, `--language`, `--speed` | Kokoro voice (default `af_heart`), language, 0.5 to 2.0 |
| `--device` | `NPU` (default), `GPU` or `CPU` |
| `--audio-device` | A sink from `pactl list short sinks`, or an ALSA device |
| `--kind` | Read `--file` as `plain`, `markdown` or `document` (one paragraph or cell a line, as vinoSearch extracts) |
| `--model-dir` | Use your own model directory, unchecked |

In markdown, headings, list items and table rows become sentences, links keep
their text, URLs are said as their host and code blocks are skipped.
Abbreviations like `e.g.` and `Dr.` don't end a sentence. `vinospeak --help`
lists everything.

The model is fetched from Hugging Face at one pinned commit and every file is
checked against its sha256 (see [SECURITY.md](SECURITY.md)). It lives in
`~/.cache/vinospeak/`, along with OpenVINO's compiled-model cache so later runs
skip the NPU compile.

## Check this machine

```bash
vinospeak-doctor          # --json for a bug report
```

Shows the OpenVINO packages and devices, the audio player it picked, and
whether the cached model still matches its pins. With
[ovfetch](https://github.com/karanshukla/ovfetch) on your `PATH` it also checks
your OpenVINO version against your NPU driver. If your distro's driver has no
NPU compiler (Fedora's `intel-npu-driver` 1.32.0), `ovfetch npu install`
fetches Intel's into your home directory and vinoSpeak loads it from there.

## Python

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles once; reuse it
audio = synth.synthesize("Hello.")  # audio.samples (float32), audio.sample_rate
speech = synth.speak("A long passage.")  # plays in the background
speech.stop()  # or speech.wait()
```

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md). `uv run poe check` runs everything CI
does; none of it needs OpenVINO or an NPU.
