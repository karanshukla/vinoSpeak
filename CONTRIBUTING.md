# Contributing

vinoSpeak is developed on a single Intel NPU laptop, so hardware reports from
other machines are especially valuable. Please attach `vinospeak-doctor --json`
to any bug report.

## Setup

Tests do not require OpenVINO or an NPU. To install only the test and lint
dependencies (matching CI):

```bash
uv sync --locked --only-group dev
.venv/bin/python -m pytest --cov    # branch coverage; CI fails under 96%
```

To install the full runtime and run synthesis locally:

```bash
uv sync          # .python-version pins 3.14 because OpenVINO has no 3.15 wheel yet
uv run vinospeak "Hello." --device CPU --output hello.wav
```

## Before you push

Run the full check suite:

```bash
uv run poe check    # ruff, ruff format --check, mypy, pytest
```

Guidelines:

- **Hardware-free tests:** Unit tests mock the pipeline, `openvino`, and
  `ovfetch` subprocess calls so they run without OpenVINO or an NPU. If a change
  can only be verified on real hardware, state what you tested in the PR instead
  of faking a test.
- **Comments and docs:** Prefer extracting, naming, or pinning a rule with a
  test pair before adding a comment, and link the test from the code it governs.
  See [docs/comment-style.md](docs/comment-style.md); `tests/test_doc_links.py`
  and `gui/tests/doc_links.rs` verify those links in CI. Longer rationale and
  measurements belong in [docs/architecture.md](docs/architecture.md).
- **Measured claims:** Always include the date alongside any measurement.
- **Commits:** Use conventional commit prefixes (`feat:`, `fix:`, `docs:`, ...)
  so `git cliff` can generate the changelog.

## Releasing

```bash
git cliff -c cliff.toml --unreleased --tag vX.Y.Z --prepend CHANGELOG.md   # review/edit, then commit
uv run bump-my-version bump minor                                          # updates version, commits, and tags vX.Y.Z
git push --follow-tags
```

Pushing the tag triggers `release.yml`, which verifies the tag against
`pyproject.toml`, builds the sdist and wheel, publishes to PyPI via Trusted
Publishing (using the `pypi` environment with no static token), and creates a
GitHub Release using that version's section from `CHANGELOG.md`.

Because PyPI does not allow re-uploading a version, commit the changelog section
before bumping so the tag includes it. Always use `--prepend` rather than `-o`
so manual edits to older changelog entries are preserved.

## Updating model pins

```bash
python scripts/update_pins.py [REVISION]
```

This script downloads the specified revision, cross-checks LFS hashes against
the Hugging Face Hub, and updates `vino_speak/model_pins.json`. Review the diff
before committing (see [SECURITY.md](SECURITY.md)).

## Updating OpenVINO

`pyproject.toml` pins `openvino` and `openvino-genai` to exact versions shared
with vinoPop and vinoWhisper so programs sharing an NPU never run mismatched
OpenVINO releases. Update all three repositories together, then run
`vinospeak-doctor` on an NPU machine to verify whether `ovfetch` still considers
the new version within your driver's supported range.

## Embedding in another Python program

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles the model once; reuse it
audio = synth.synthesize("Hello.")  # Audio(samples: float32 array, sample_rate: int)
```

`Synthesizer` is thread-safe (calls are serialized) and keeps no state between
calls beyond loaded voices. Importing `vino_speak` does not import OpenVINO or
NumPy, so startup overhead stays minimal.
