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

One line, no sudo, everything in your home:

```bash
curl -fsSL https://github.com/karanshukla/vinoSpeak/releases/latest/download/install.sh | bash
vinospeak "Hello."
```

Add `-s -- --gui` after `bash` for the window too. `vinospeak-upgrade` upgrades
it, and `vinospeak-upgrade --uninstall` removes it all, the downloaded model
included.

Or with [uv](https://docs.astral.sh/uv/), Python 3.11 to 3.14:

```bash
uv tool install --python 3.14 vinospeak
```

From source, the window, Fedora Atomic, NixOS and which package has an audio
player: [docs/install.md](docs/install.md).

## What it needs

| | |
|---|---|
| Linux | Python comes with uv; espeak-ng and libsndfile come in the wheels |
| An audio player | `pw-play`, `paplay` or `aplay`, or use `--output` |
| For `--device NPU` | An Intel NPU and its driver. `--device CPU` needs no driver at all |
| Disk | About 160 MB for the model, downloaded once into `~/.cache/vinospeak/` |

## Use

```bash
vinospeak "Hello from vinoSpeak."                  # play it
vinospeak "Hello." --output hello.wav              # save a WAV
vinospeak --file notes.md                          # read a file aloud
vinospeak --file notes.md --print-text             # see what it would say, no model load
some-command | vinospeak --file -                  # plain text from stdin
vinospeak --file 2026-10-27-140512.vwsession       # a vinoWhisper summary
vinospeak "Hola." --voice ef_dora --language es --speed 1.2 --device CPU
```

| Option | What it does |
|---|---|
| `--voice`, `--language`, `--speed` | Kokoro voice (default `af_heart`), language, 0.5 to 2.0 |
| `--device` | `NPU` (default), `GPU` or `CPU` |
| `--audio-device` | A sink from `pactl list short sinks`, or an ALSA device |
| `--kind` | Read `--file` as `plain`, `markdown` or `document` |
| `--part` | `summary` or `transcript` of a vinoWhisper session |
| `--model-status`, `--delete-model` | Show or delete the downloaded model |
| `--model-dir` | Use your own model directory, unchecked |

`vinospeak --help` lists everything, and `vinospeak-doctor` checks what this
machine has (`--json` for a bug report).

From Python:

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles once; reuse it
audio = synth.synthesize("Hello.")  # audio.samples (float32), audio.sample_rate
```

## Docs

| | |
|---|---|
| [docs/install.md](docs/install.md) | Every way to install it, the window, distro notes |
| [docs/usage.md](docs/usage.md) | How files are read, sessions, the model on disk, the doctor, the Python API |
| [docs/architecture.md](docs/architecture.md) | Why it is built this way, with the measurements |
| [gui/README.md](gui/README.md) | The window |
| [SECURITY.md](SECURITY.md) | What leaves your machine (nothing), and how the model download is verified |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Setup, tests, releasing. `uv run poe check` runs what CI does; none of it needs OpenVINO or an NPU |
