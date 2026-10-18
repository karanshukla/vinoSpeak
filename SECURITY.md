# Security

## What vinoSpeak does with your text

Nothing leaves the machine. Text is synthesized by a model running on local
hardware and written to a WAV file you name. There is no network call in the
synthesis path, no telemetry, and no account. OpenVINO ships Intel's usage
telemetry (Google Analytics), on unless you opt out; vinoSpeak never loads it.
Your `~/intel/openvino_telemetry` consent file is left alone. The only outbound traffic the
project makes is package installation and the one-time model download from
Hugging Face, both of which you trigger explicitly; the second is checked
against pinned digests (below).

The WAV is as sensitive as the text it was made from. It is written with your
default umask, and nothing deletes it.

## The model download is verified

`vino_speak/model_pins.json` pins `OpenVINO/Kokoro-82M-int8-ov` to one commit
and records the sha256 and size of every file the pipeline can read (66 files,
~160 MB). The download requests exactly that revision and those files, then
hashes each one; anything missing, resized or different stops the run and the
directory is not used. Later runs compare file sizes (cheap enough for every
run), and `vinospeak-doctor` repeats the full hash check.

What that does and does not give you:

- **It protects against the repository changing after the pin**: a force-push,
  a replaced file, a compromised CDN edge. It does not protect against the
  repository being malicious at the pinned commit, since nobody but the
  maintainer reviewed it. Read the model card and the diff before re-pinning.
- **The hashes come from the same host.** The two large LFS files (the weights
  and the voice files) were cross-checked against the sha256 the Hub API
  reports for them, which is the Hub's own record, not an independent source.
  The rest were hashed after download.
- **`--model-dir` with a populated directory is trusted as given.** It is not
  compared with the pins, so a model you converted yourself works. Only point
  it at files you trust: the `.bin` voices are read as raw floats and the
  OpenVINO IR is parsed by OpenVINO.
- Re-pinning is `python scripts/update_pins.py [REVISION]`, deliberately a
  script and not automatic. The diff it produces is a list of hashes, and it
  is meant to be read in review.

## Voice names cannot escape the model directory

A voice is looked up as `voices/<name>.bin` inside the model directory, and the
name must match a Kokoro identifier (`xx_name`), so `../` and absolute paths
are refused before any file is opened.

## ovfetch is advisory

`vinospeak-doctor` runs `ovfetch detect` if it is on your `PATH`, to compare
your OpenVINO version with the range recorded for your NPU driver. It reads
the output as data and changes nothing. vinoSpeak never runs `ovfetch install`:
OpenVINO comes from pip, pinned in `pyproject.toml`.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting on this repository
(Security tab → Report a vulnerability) rather than a public issue. This is a
personal project, so expect a best-effort, spare-time response rather than an
SLA.
