# Install

## The installer

One line, no sudo, everything in your home:

```bash
curl -fsSL https://github.com/karanshukla/vinoSpeak/releases/latest/download/install.sh | bash
vinospeak "Hello."
```

It checks the latest release's wheel against its `SHA256SUMS` and installs it
with [uv](https://docs.astral.sh/uv/), fetching uv itself (hash-pinned) if you
don't have it. Add `-s -- --gui` after `bash` for the window too.

| To | Run |
|---|---|
| Upgrade | `vinospeak-upgrade`, or **Check for updates** on the window's About page |
| Uninstall, model included (uv stays) | `vinospeak-upgrade --uninstall` |

## With uv

Python 3.11 to 3.14:

```bash
uv tool install --python 3.14 vinospeak
```

`--python 3.14` because OpenVINO 2026.4.1 has no 3.15 wheels yet. OpenVINO is
pinned to the same release as vinoPop and vinoWhisper, so programs sharing one
NPU never run mismatched runtimes.

`uv tool uninstall vinospeak` leaves the model behind; run
`vinospeak --delete-model` first.

## From source

For the exact versions in `uv.lock`, hashes included. `--locked` fails if the
lock is out of date:

```bash
git clone https://github.com/karanshukla/vinoSpeak && cd vinoSpeak
uv sync --locked
uv run vinospeak "Hello."
```

To put that checkout's `vinospeak` on your `PATH` with the lock's versions as
constraints (`uv tool install` doesn't read `uv.lock` by itself):

```bash
uv export --locked --no-dev --no-emit-project --no-hashes -o constraints.txt
uv tool install --constraints constraints.txt .
```

## The window

The window isn't on PyPI. The installer's `--gui` takes the release's prebuilt
x86_64 binary; from a checkout you build it with [Rust](https://rustup.rs).
This puts it in your app menu, using the `vinospeak` on your `PATH` or in
`~/.local/bin`:

```bash
gui/install.sh                                             # --uninstall removes it
VINOSPEAK=.venv/bin/vinospeak gui/install.sh               # or the checkout's vinospeak
```

Finding files in it needs [vinoSearch](https://github.com/karanshukla/vinoSearch)
running. See [gui/README.md](../gui/README.md).

## Distro notes

**Fedora Atomic / openSUSE Aeon**: nothing to layer for vinoSpeak itself. uv
brings its own Python and the wheels carry espeak-ng and libsndfile. Only the
NPU driver (for `--device NPU`) and an audio player, if you lack one, are
system packages.

**NixOS**: uv's Python and the window need the standard dynamic linker, so
enable `programs.nix-ld`; without it `install.sh` stops and says so.

## An audio player

vinoSpeak plays through `pw-play`, `paplay` or `aplay`, whichever it finds
first. With none of them, install one or use `--output`:

| Distro | `pw-play` | `paplay` |
|---|---|---|
| Fedora | `pipewire-utils` | `pulseaudio-utils` |
| Ubuntu / Debian | `pipewire-bin` | `pulseaudio-utils` |
| Arch | `pipewire` | `libpulse` |
| openSUSE | `pipewire-tools` | `pulseaudio-utils` |

`pw-play` needs a PipeWire newer than 1.0 to take raw audio; an older one is
skipped for `paplay`. `vinospeak-doctor` says which player it picked.
