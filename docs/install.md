# Install

## The installer

Install into your home directory without `sudo`:

```bash
curl -fsSL https://github.com/karanshukla/vinoSpeak/releases/latest/download/install.sh | bash
vinospeak "Hello."
```

The script verifies the latest release wheel against its `SHA256SUMS` file and
installs it with [uv](https://docs.astral.sh/uv/), downloading a hash-pinned
copy of `uv` first if it is not already installed. Pass `-s -- --gui` after
`bash` to install the desktop window too.

The installer sets `UV_PYTHON_PREFERENCE=only-managed` so `uv` uses its own
managed Python instead of `/usr/bin/python3`. On Fedora 43+, where the system
Python is also 3.14, a future OS upgrade to 3.15 would otherwise break the
tool's virtual environment. Using the environment variable instead of
`--managed-python` ensures older `uv` binaries already on `PATH` still work.

| Task | Command |
|---|---|
| Upgrade | `vinospeak-upgrade`, or **Check for updates** on the window's About page |
| Uninstall (including the model; keeps `uv`) | `vinospeak-upgrade --uninstall` |

## With uv

Requires Python 3.11–3.14:

```bash
uv tool install --python 3.14 vinospeak
```

Python 3.14 is pinned because OpenVINO 2026.4.1 does not yet ship Python 3.15
wheels. OpenVINO is also pinned to the same release used by vinoPop and
vinoWhisper so applications sharing an NPU run matching runtimes.

`vinospeak --version` prints `vinospeak <version>` for release installs (PyPI or
the release installer) and `vinospeak dev` for installs from a source checkout.

Note that `uv tool uninstall vinospeak` leaves the downloaded model on disk; run
`vinospeak --delete-model` first if you want to remove it as well.

## From source

Clone the repository and sync against `uv.lock` (including hashes; `--locked`
fails if the lockfile is out of date):

```bash
git clone https://github.com/karanshukla/vinoSpeak && cd vinoSpeak
uv sync --locked
uv run vinospeak "Hello."
```

Because `uv tool install` does not read `uv.lock` directly, export constraints
first if you want to put the checkout's `vinospeak` on your `PATH` with the
locked dependency versions:

```bash
uv export --locked --no-dev --no-emit-project --no-hashes -o constraints.txt
uv tool install --constraints constraints.txt .
```

## The window

The desktop window is not published to PyPI. Passing `--gui` to the release
installer downloads the prebuilt `x86_64` binary. To build it from a checkout
instead, install [Rust](https://rustup.rs) and run `gui/install.sh` to add it to
your application menu (it uses `vinospeak` from `PATH` or `~/.local/bin` by
default):

```bash
gui/install.sh                                             # --uninstall removes it
VINOSPEAK=.venv/bin/vinospeak gui/install.sh               # or use the checkout's vinospeak
```

File search inside the window requires
[vinoSearch](https://github.com/karanshukla/vinoSearch) to be running. See
[gui/README.md](../gui/README.md) for details.

## Distro notes

- **Fedora Atomic / openSUSE Aeon:** No packages need to be layered for
  vinoSpeak itself. `uv` manages its own Python, and the wheels bundle
  `espeak-ng` and `libsndfile`. Only the Intel NPU driver (for `--device NPU`)
  and an audio player (if missing) come from system packages.
- **NixOS:** Both `uv`'s Python and the desktop window require the standard
  dynamic linker, so enable `programs.nix-ld`. Without it, `install.sh` exits
  with an explanatory message.

## Audio players

vinoSpeak checks for `pw-play`, `paplay`, and `aplay` in that order and uses the
first one available. If none are installed, install one from the table below or
save to a file with `--output`:

| Distro | `pw-play` | `paplay` |
|---|---|---|
| Fedora | `pipewire-utils` | `pulseaudio-utils` |
| Ubuntu / Debian | `pipewire-bin` | `pulseaudio-utils` |
| Arch | `pipewire` | `libpulse` |
| openSUSE | `pipewire-tools` | `pulseaudio-utils` |

`pw-play` requires PipeWire newer than 1.0 to accept raw audio input; older
versions are skipped in favor of `paplay`. Run `vinospeak-doctor` to see which
player vinoSpeak selected.
