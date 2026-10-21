<p align="center">
  <img src="https://raw.githubusercontent.com/karanshukla/vinoSpeak/main/docs/assets/vinospeak.svg" width="112" alt="The vinoSpeak mark: a white speaker with two green sound waves on the dark vino tile">
</p>

# vinoSpeak

[![PyPI](https://img.shields.io/pypi/v/vinospeak?logo=pypi&logoColor=white)](https://pypi.org/project/vinospeak/)
[![Python](https://img.shields.io/pypi/pyversions/vinospeak)](https://pypi.org/project/vinospeak/)
[![License](https://img.shields.io/badge/license-MIT-blue)](https://github.com/karanshukla/vinoSpeak/blob/main/LICENSE)
[![CI](https://github.com/karanshukla/vinoSpeak/actions/workflows/ci.yml/badge.svg)](https://github.com/karanshukla/vinoSpeak/actions/workflows/ci.yml)
[![Ruff](https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json)](https://github.com/astral-sh/ruff)

Intel NPU-powered text-to-speech using the Kokoro-82M INT8 OpenVINO model.
Text goes in, a WAV comes out, and nothing leaves your machine after the
one-time model download.

## Requirements

- Python 3.11 or newer
- An Intel NPU with a compatible driver. GPU and CPU also work
  (`--device GPU`, `--device CPU`).
- Internet access on first run to download the model (~160 MB)

OpenVINO is pinned to the release vinoPop and vinoWhisper use, so programs
sharing one NPU never run mismatched runtimes.

## Install

From PyPI, with [uv](https://docs.astral.sh/uv/):

```bash
uv tool install --python 3.14 vinospeak
vinospeak "Hello."
```

`--python 3.14` because OpenVINO 2026.4.1 has no wheels for 3.15 yet, and uv
otherwise uses your default Python. OpenVINO is pinned exactly; everything else resolves to the newest release
that fits. To install exactly what `uv.lock` pins, hashes included, use a
checkout. `--locked` fails if the lock is out of date:

```bash
git clone https://github.com/karanshukla/vinoSpeak && cd vinoSpeak
uv sync --locked
uv run vinospeak "Hello."
```

To put that checkout's `vinospeak` on your PATH with the lock's versions as
constraints (`uv tool install` doesn't read `uv.lock` by itself):

```bash
uv export --locked --no-dev --no-emit-project --no-hashes -o constraints.txt
uv tool install --constraints constraints.txt .
```

## Synthesize speech

The first run downloads `OpenVINO/Kokoro-82M-int8-ov` from Hugging Face, at one
pinned commit, and checks every file against its sha256 (see
[SECURITY.md](SECURITY.md)). The model is cached under
`${XDG_CACHE_HOME:-~/.cache}/vinospeak/` by default.

```bash
vinospeak "Hello from vinoSpeak."                     # plays it on your speakers
vinospeak "Hello from vinoSpeak." --output hello.wav  # saves a WAV instead
```

Playback goes through `pw-play` (PipeWire, what Wayland desktops run), falling
back to `paplay` or `aplay`.

The default inference device is the Intel NPU. Choose a different OpenVINO
device, voice, language, speed, or model directory with the corresponding
options:

```bash
vinospeak "A sample sentence." \
  --device NPU \
  --voice af_heart \
  --language en-us \
  --speed 1.0 \
  --model-dir ./kokoro-model \
  --output speech.wav
```

Use `vinospeak --help` to see all options. The audio uses the sample rate
reported by OpenVINO GenAI.

## Check this machine

```bash
vinospeak-doctor          # --json for a bug report
```

Reports the OpenVINO packages and devices, whether the cached model still
matches its pins, and, if [ovfetch](https://github.com/karanshukla/ovfetch) is
on your `PATH`, whether your OpenVINO version is inside the range recorded as
working with your NPU driver. Install it with `cargo install ovfetch --locked`.
ovfetch is advisory here: it never changes what gets installed or run.

## Use it from Python

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles the model once; reuse it
audio = synth.synthesize("Hello.")  # audio.samples (float32), audio.sample_rate

speech = synth.speak("A long passage.")  # returns at once, plays in the background
speech.stop()  # cuts it off; speech.wait() blocks until it finishes instead
```

`speak()` synthesizes the first sentence on its own and plays it while the
rest is synthesized in chunks that at most double in size, so speech starts
after about 1.5 s on the NPU however long the text is.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md). `uv run poe check` runs everything CI
does; none of it needs OpenVINO or an NPU.
