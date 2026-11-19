<p align="center">
  <img src="https://raw.githubusercontent.com/karanshukla/vinoSpeak/main/docs/assets/vinospeak.svg" width="112" alt="The vinoSpeak mark: a white speaker with two green sound waves on the dark vino tile">
</p>

# vinoSpeak

[![PyPI](https://img.shields.io/pypi/v/vinospeak?logo=pypi&logoColor=white)](https://pypi.org/project/vinospeak/)
[![Python](https://img.shields.io/pypi/pyversions/vinospeak)](https://pypi.org/project/vinospeak/)
[![License](https://img.shields.io/badge/license-MIT-blue)](https://github.com/karanshukla/vinoSpeak/blob/main/LICENSE)
[![CI](https://github.com/karanshukla/vinoSpeak/actions/workflows/ci.yml/badge.svg)](https://github.com/karanshukla/vinoSpeak/actions/workflows/ci.yml)
[![Ruff](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json)](https://github.com/astral-sh/ruff)

Local text-to-speech on the Intel NPU using Kokoro-82M INT8 through OpenVINO
GenAI. Pass text, a file, or stdin and vinoSpeak reads it aloud or saves audio.
After a one-time ~160 MB model download, all synthesis runs locally with
OpenVINO telemetry disabled.

- **Text, files, and stdin:** Reads plain text, cleans up Markdown for
  listening, and supports PDFs, Word documents, and slides via
  [vinoSearch](https://github.com/karanshukla/vinoSearch)'s extracted text.
- **vinoWhisper sessions:** Reads either the summary or the transcript of a
  [vinoWhisper](https://github.com/karanshukla/vinoWhisper) `.vwsession` file.
- **Low latency:** Starts speaking in about 1.5 s on the NPU regardless of text
  length by playing the first sentence while synthesizing the rest.
- **41 Kokoro voices:** Covers US/UK English, Spanish, French, Hindi, Italian,
  and Brazilian Portuguese, with 0.5x–2.0x speed control and multi-voice
  blending.
- **Optional local translation:** Uses SmolLM3-3B (the same 1.7 GB opt-in model
  used by vinoWhisper) to translate text into Spanish, French, Italian,
  Portuguese, or English as it reads.
- **SSML and custom pronunciations:** Supports a subset of SSML (pauses, voice
  and speed changes, aliases) plus a TOML lexicon for custom word respellings.
- **Speakers or audio files:** Plays through PipeWire, PulseAudio, or ALSA, or
  writes WAV, FLAC, OGG, Opus, or MP3 files with timed SRT or WebVTT subtitles.
- **Flexible hardware:** Runs on Intel NPU, GPU, or CPU (CPU requires no extra
  driver).
- **Local OpenAI-compatible HTTP API:** `vinospeak --http` serves
  `/v1/audio/speech` on localhost for Open WebUI, Home Assistant, or any OpenAI
  TTS client.
- **Desktop GUI and Python API:** Includes a proof-of-concept
  [desktop window](gui/) integrated with vinoSearch, as well as a Python library
  API.

<p align="center"><img src="docs/assets/gui.svg" width="800" alt="The vinoSpeak window. Typing &quot;how often is the lens cleaned&quot; finds lighthouse-log.pdf through vinoSearch; Read aloud starts speaking it and the status bar says so. Then a vinoWhisper session, Harbour safety meeting, is picked and its summary is read aloud."></p>

## Install

Install into your home directory without `sudo`:

```bash
curl -fsSL https://github.com/karanshukla/vinoSpeak/releases/latest/download/install.sh | bash
vinospeak "Hello."
```

Pass `-s -- --gui` after `bash` to install the desktop window as well. Use
`vinospeak-upgrade` to update, or `vinospeak-upgrade --uninstall` to remove
vinoSpeak along with the downloaded model.

You can also install with [uv](https://docs.astral.sh/uv/) on Python 3.11–3.14:

```bash
uv tool install --python 3.14 vinospeak
```

See [docs/install.md](docs/install.md) for source builds, the desktop window,
Fedora Atomic and NixOS notes, and audio player packages.

## Requirements

| Requirement | Details |
|---|---|
| Linux | Python is managed by uv; `espeak-ng` and `libsndfile` are bundled in the wheels |
| Audio player | `pw-play`, `paplay`, or `aplay` for speaker playback (not required when using `--output`) |
| Intel NPU (optional) | Required for `--device NPU` along with the Intel NPU driver; `--device CPU` works without any accelerator driver |
| Disk space | ~160 MB for the Kokoro model in `~/.cache/vinospeak/`; an additional 1.7 GB in `~/.local/share/vinowhisper/models/` if you enable translation (shared with vinoWhisper) |

## Usage

```bash
vinospeak "Hello from vinoSpeak."                  # play it
vinospeak "Hello." --output hello.wav              # save a WAV (or .flac .ogg .opus .mp3)
vinospeak --file notes.md --output notes.mp3 --subtitles notes.srt
vinospeak --file notes.md                          # read a file aloud
vinospeak --file notes.md --print-text             # see what it would say, no model load
some-command | vinospeak --file -                  # plain text from stdin
vinospeak --file 2026-10-27-140512.vwsession       # a vinoWhisper summary
vinospeak '<speak>Wait.<break time="1s"/><voice name="bm_george">Done.</voice></speak>'
vinospeak "Hola." --voice ef_dora --speed 1.2 --device CPU    # Spanish: the voice's language
vinospeak --file notes.md --translate fr-fr                      # in French (after --download-translation-model)
vinospeak "Hello." --voice "af_heart(2)+bm_george(1)"            # a blend of two voices
```

| Option | Description |
|---|---|
| `--voice`, `--speed` | Kokoro voice (default `af_heart`) or a blend like `af_heart(2)+bm_george(1)`; speed from `0.5` to `2.0` |
| `--list-voices` | List all available voices with language and accent |
| `--language` | Override the voice's default language |
| `--output`, `--subtitles` | Save audio instead of playing (format chosen by file suffix); write sentence-timed `.srt` or `.vtt` subtitles |
| `--device` | Inference device: `NPU` (default), `GPU`, or `CPU` |
| `--audio-device` | Target sink from `pactl list short sinks`, or an ALSA device name |
| `--kind` | Interpret `--file` as `plain`, `markdown`, or `document` |
| `--translate`, `--download-translation-model` | Translate into `es`, `fr-fr`, `it`, `pt-br`, `en-us`, or `en-gb` before speaking; the translation model is only downloaded when requested |
| `--lexicon` | Use a custom TOML pronunciation file instead of `~/.config/vinospeak/lexicon.toml` |
| `--part` | Choose `summary` or `transcript` when reading a vinoWhisper session |
| `--http [HOST:]PORT` | Serve OpenAI's `/v1/audio/speech` API (default `127.0.0.1:8880`) |
| `--model-status`, `--delete-model` | Inspect or delete the cached Kokoro model |
| `--model-dir` | Load an unverified local model directory |

Run `vinospeak --help` for the full option list, or `vinospeak-doctor` to check
your environment (`--json` for bug reports).

### Python API

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles once; reuse it
audio = synth.synthesize("Hello.")  # audio.samples (float32), audio.sample_rate
```

## Documentation

| Document | Contents |
|---|---|
| [docs/install.md](docs/install.md) | Installation methods, desktop window setup, distro notes, and audio player packages |
| [docs/usage.md](docs/usage.md) | File parsing, translation, SSML, lexicons, sessions, HTTP server, model management, doctor, and Python API |
| [docs/architecture.md](docs/architecture.md) | Module layout, design rationale, and performance measurements |
| [gui/README.md](gui/README.md) | Desktop window usage, controls, and headless testing |
| [SECURITY.md](SECURITY.md) | Privacy model, HTTP server safeguards, and model hash verification |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Development setup, tests (`uv run poe check`, which requires no OpenVINO or NPU), and release process |
