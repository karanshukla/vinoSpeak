# Contributing

vinoSpeak is built on one Intel NPU laptop. The most useful thing you can bring
is what it does on *your* hardware, since that is what I cannot test from here.
Attach `vinospeak-doctor --json` to any report.

## Setup

Nothing that avoids the model needs OpenVINO. This is what CI does:

```bash
uv sync --locked --only-group dev
.venv/bin/python -m pytest --cov    # branch coverage; CI fails under 96%
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

## Releasing

```bash
uv run bump-my-version bump minor                       # writes the version, commits, tags vX.Y.Z
git cliff -c cliff.toml --tag vX.Y.Z -o CHANGELOG.md    # then hand-edit, commit
git push --follow-tags
```

The tag runs `release.yml`: it checks the tag matches `pyproject.toml`, builds
the sdist and wheel, publishes to PyPI by Trusted Publishing (the `pypi`
environment, no token), then creates the GitHub Release with notes from the
`CHANGELOG.md` section of that version. A version can't be re-uploaded to PyPI,
so the changelog section must exist before the tag is pushed.

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
