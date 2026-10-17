# Contributing

vinoSpeak is built on one Intel NPU laptop. The most useful thing you can bring
is what it does on *your* hardware, since that is what I cannot test from here.
Attach `vinospeak-doctor --json` to any report.

## Setup

Nothing that avoids the model needs OpenVINO. This is what CI does:

```bash
uv venv
uv pip install --group dev numpy
.venv/bin/python -m pytest
```

For the real thing:

```bash
uv sync          # .python-version picks 3.14: OpenVINO has no 3.15 wheel yet
uv run vinospeak "Hello." --device CPU --output hello.wav
```

## Before you push

```bash
uv run poe check    # ruff, ruff format --check, mypy, pytest
```

- Tests run with no NPU and no OpenVINO: the pipeline, `openvino` and the
  `subprocess` call to ovfetch are faked. If a change can only be verified on
  hardware, say so in the PR instead of faking a test.
- Keep code comments to one line, and only where the code would invite a wrong
  "fix".
- Measured claims carry a date.
- Commits are conventional (`feat:`, `fix:`, `docs:`, ...); the changelog is
  generated from them with `git cliff`.

## Changing the model

```bash
python scripts/update_pins.py [REVISION]
```

Downloads that revision, cross-checks the LFS hashes against the Hub, and
rewrites `vino_speak/model_pins.json`. Read the diff; see SECURITY.md.

## Changing OpenVINO

`pyproject.toml` pins `openvino` and `openvino-genai` exactly, to the release
vinoPop and vinoWhisper use: two programs on one NPU must not run different
OpenVINO releases. Bump all three together, then run `vinospeak-doctor` on an
NPU to see whether `ovfetch` still reports the version inside your driver's
range.

## Using it from another program

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles the model once; reuse it
audio = synth.synthesize("Hello.")  # Audio(samples: float32 array, sample_rate: int)
```

`Synthesizer` is thread-safe (calls are serialized) and keeps no state between
calls beyond the loaded voices. Importing `vino_speak` imports neither OpenVINO
nor numpy, so it is cheap to import at start-up.
