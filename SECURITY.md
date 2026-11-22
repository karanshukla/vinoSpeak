# Security

## Local processing and privacy

All speech synthesis runs locally on your hardware and plays directly to your
speakers, or writes to a file when you pass `--output` (no audio is saved by
default). Synthesis requires no account or network access and sends no
telemetry.

OpenVINO bundles Intel usage telemetry (Google Analytics) that defaults to
enabled unless opted out; vinoSpeak blocks `openvino_telemetry` from loading
while leaving your `~/intel/openvino_telemetry` consent file untouched for other
OpenVINO tools. The only outbound network activity is package installation and
explicitly triggered model downloads from Hugging Face, which are verified
against pinned SHA-256 digests.

Any audio file written via `--output` is as sensitive as its input text. It is
created with your default umask and is not deleted automatically.

## HTTP server (`vinospeak --http`)

`--http` is the only mode that listens on a network socket. Any client that can
reach the port can request synthesized audio (requests return audio bytes; the
server never reads local files or plays audio on your speakers). Safeguards:

- **Loopback by default:** Binds to `127.0.0.1` only. Binding to any other
  address is rejected unless `VINOSPEAK_API_KEY` is set, in which case every
  request must include `Authorization: Bearer <key>` (verified in constant
  time). Because the key travels over plain HTTP, place the server behind TLS or
  restrict it to a trusted network when serving off-host.
- **Browser protection:** When running without an API key, requests whose `Host`
  header is not a loopback address are rejected (preventing DNS rebinding). The
  server also requires `application/json` request bodies, which browsers do not
  send cross-origin without a CORS preflight that the server does not answer.
- **Resource bounds:** Requests are capped at 1 MiB body size, 100,000 input
  characters, Kokoro's supported speed range, and 10 s per SSML `<break>`. Logs
  to stderr record only the path and status code, never the input text.

## Model download verification

`vino_speak/model_pins.json` pins `OpenVINO/Kokoro-82M-int8-ov` to a specific
commit and lists the SHA-256 digest and byte size of every file the pipeline can
read (66 files, ~160 MB). Downloads request that exact revision and file set,
then hash every file; if any file is missing, resized, or mismatched, the run
aborts and failed files are deleted so the next run downloads them again.

After all files match, a `.vinospeak-verified` stamp is written. Subsequent runs
perform a fast file-size check; if the stamp is absent or from older pins, all
files are hashed in full. `vinospeak-doctor` always repeats the full SHA-256
verification.

Scope and limitations:

- **Tamper detection after pinning:** Pinning protects against post-pin
  repository changes (force-pushes, replaced files, or a compromised CDN edge).
  It does not guard against malicious files present at the pinned commit itself,
  as only the maintainer reviewed it; inspect the model card and diff before
  re-pinning.
- **Hash provenance:** The two large LFS files (model weights and voice files)
  were cross-checked against the SHA-256 values reported by the Hugging Face Hub
  API (the Hub's own record, not an independent third party). The remaining
  files were hashed after download.
- **Custom `--model-dir`:** A populated directory passed to `--model-dir` is
  trusted as-is without pin checks so locally converted models work. Only pass
  directories you trust, since `.bin` voices are read as raw floats and OpenVINO
  IR files are parsed by OpenVINO.
- **Manual re-pinning:** `python scripts/update_pins.py [REVISION]` is
  deliberately manual so its hash diff can be reviewed before committing.

## Translation model

`--download-translation-model` (or **Download…** on the GUI's **Settings** page)
fetches `arankays/SmolLM3-3B-int4-cw-ov` (vinoWhisper's export of SmolLM3-3B),
pinned to a specific revision with per-file SHA-256 digests in
`vino_speak/translation_pins.json` and verified the same way as Kokoro. It is
never downloaded automatically, and translation runs locally on your machine
without sending text over the network.

## Voice path restriction

Voices are resolved as `voices/<name>.bin` inside the model directory. Voice
names must match a Kokoro identifier (`xx_name`), rejecting `../` traversal and
absolute paths before any file is opened.

## ovfetch integration

If `ovfetch` is on your `PATH`, `vinospeak-doctor` runs `ovfetch detect` to
compare your OpenVINO version against the range recorded for your NPU driver. It
only reads the output and makes no changes. vinoSpeak never invokes
`ovfetch install`; OpenVINO comes from pip as pinned in `pyproject.toml`.

The only library vinoSpeak loads outside pip is the NPU compiler when installed
into `~/.local/lib/ovfetch-npu` via `ovfetch npu install` (ovfetch 0.6.0).
Before opening the NPU, vinoSpeak `dlopen`s each library listed in that prefix's
`ovfetch-preload.txt` in order. Because ovfetch verifies the upstream tarball
against its ledger and the prefix is stored in your home directory, this carries
the same trust model as packages in your virtualenv. vinoSpeak never writes to
that prefix, and loads nothing if the prefix does not exist.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting on this repository
(**Security** tab → **Report a vulnerability**) instead of opening a public
issue. This is a personal project, so expect a best-effort, spare-time response
rather than a formal SLA.
