#!/usr/bin/env bash
# Install or upgrade vinoSpeak from a GitHub release, for this user:
#
#   curl -fsSL https://github.com/karanshukla/vinoSpeak/releases/latest/download/install.sh | bash
#
# Options go after `bash -s --`, e.g. `| bash -s -- --gui`:
#
#   vN          that release instead of the newest
#   --gui       also the window (find a file with vinoSearch, listen to it)
#   --uninstall remove vinospeak, the window and this script's copy
#
# It downloads the release's SHA256SUMS, then the wheel (and the window's
# tarball) and refuses anything not listed there or not matching. The wheel
# goes in with `uv tool install` (uv itself comes from its own release,
# checked against a hash pinned below, if it isn't installed). The window
# goes in ~/.local/bin with its app menu entry. It also keeps a checked copy
# of this script as vinospeak-upgrade, which the window's Update button runs
# with VINO_UPDATE_TAG set; an upgrade keeps the window if it's installed.
#
# No sudo: everything goes in your home. From a checkout, see the README.

# Everything is in main, called on the last line: piped into bash, a download
# cut short must not run half a script.
set -euo pipefail

REPO="karanshukla/vinoSpeak"
GUI_ASSET="vinospeak-gui-x86_64-linux.tar.gz"
# OpenVINO 2026.4.1 has no 3.15 wheels; the same pin as the README.
PYTHON="3.14"

# Bumped by hand, in a reviewed commit, as in vinoWhisper's installer: the
# binary it fetches is run as you. Bump: gh api repos/astral-sh/uv/releases/tags/$UV_VERSION \
#   -q '.assets[]|select(.name|test("linux-musl.tar.gz$"))|"\(.digest) \(.name)"'
UV_VERSION="0.12.20"
UV_SHA256_X86_64="14114d66a094f1907af0fcbc863f34226bbb7f5e430e13dce6676ba40dcc6891"

BIN="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DESKTOP="$DATA/applications/dev.vinospeak.Listen.desktop"
ICON="$DATA/icons/hicolor/scalable/apps/dev.vinospeak.Listen.svg"

say() { printf '== %s\n' "$*"; }
die() { printf '✗ %s\n' "$*" >&2; exit 1; }

refresh_menu() {
    update-desktop-database -q "$DATA/applications" 2>/dev/null || true
    kbuildsycoca6 >/dev/null 2>&1 || kbuildsycoca5 >/dev/null 2>&1 || true
}

install_uv() {
    [ "$(uname -m)" = x86_64 ] || die "no pinned uv for $(uname -m); install uv (https://docs.astral.sh/uv/) and re-run"
    local name="uv-x86_64-unknown-linux-musl"
    say "Installing uv $UV_VERSION into $BIN"
    curl -fLsS --proto '=https' --tlsv1.2 -o "$WORK/uv.tar.gz" \
        "https://github.com/astral-sh/uv/releases/download/$UV_VERSION/$name.tar.gz" \
        || die "could not download uv $UV_VERSION"
    printf '%s  %s\n' "$UV_SHA256_X86_64" "$WORK/uv.tar.gz" | sha256sum --check --status \
        || die "uv $UV_VERSION doesn't match its pinned sha256; installed nothing"
    tar -xzf "$WORK/uv.tar.gz" -C "$WORK"
    install -Dm755 "$WORK/$name/uv" "$BIN/uv"
    install -Dm755 "$WORK/$name/uvx" "$BIN/uvx"
}

# fetch NAME: download a release asset into $WORK and check it against SHA256SUMS,
# where it must be listed exactly once.
fetch() {
    curl -fsSL --retry 5 --retry-delay 2 --retry-all-errors --connect-timeout 20 \
        -o "$WORK/$1" "$BASE/$1" || die "download failed: $BASE/$1"
    (cd "$WORK" \
        && awk -v f="$1" '$2 == f || $2 == "./" f || $2 == "*" f { print $1 "  " f }' SHA256SUMS > "want-$1" \
        && [ "$(wc -l < "want-$1")" -eq 1 ] \
        && sha256sum --check --strict --quiet "want-$1") \
        || die "$1 doesn't match the release's SHA256SUMS. Nothing more was installed."
}

main() {
    local tag="${VINO_UPDATE_TAG:-}" gui=0 uninstall=0 arg
    for arg in "$@"; do
        case "$arg" in
            v[0-9]*) tag="$arg" ;;
            --gui) gui=1 ;;
            --uninstall) uninstall=1 ;;
            -h|--help) sed -n '2,20p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
            *) die "unknown option: $arg" ;;
        esac
    done
    if [ -n "$tag" ] && ! [[ "$tag" =~ ^v[0-9]+(\.[0-9]+)*$ ]]; then
        die "not a release tag: '$tag'"
    fi
    [ "$(id -u)" -ne 0 ] || die "run this as yourself, not root: vinoSpeak installs into your home"

    if [ "$uninstall" = 1 ]; then
        local uv
        uv="$(command -v uv || echo "$BIN/uv")"
        [ -x "$uv" ] && "$uv" tool uninstall vinospeak 2>/dev/null || true
        rm -f "$BIN/vinospeak-gui" "$BIN/vinospeak-upgrade" "$DESKTOP" "$ICON"
        refresh_menu
        echo "vinoSpeak removed (uv and the model cache in ~/.cache/vinospeak are kept)"
        exit 0
    fi

    local tool
    for tool in curl tar sha256sum awk; do
        command -v "$tool" >/dev/null || die "missing $tool: install it, then run this again"
    done
    # An upgrade keeps the window where it's installed.
    [ -x "$BIN/vinospeak-gui" ] && gui=1
    if [ "$gui" = 1 ] && [ "$(uname -m)" != x86_64 ]; then
        die "the window's release binary is x86_64 only; build it from a checkout (gui/install.sh)"
    fi
    # uv's Python and the window are glibc-dynamic: without this (NixOS without nix-ld)
    # they fail with a bare "No such file or directory" after a half-install.
    local ld=""
    case "$(uname -m)" in
        x86_64) ld=/lib64/ld-linux-x86-64.so.2 ;;
        aarch64) ld=/lib/ld-linux-aarch64.so.1 ;;
    esac
    [ -z "$ld" ] || [ -e "$ld" ] ||
        die "$ld is missing: this needs a standard dynamic linker (on NixOS, enable programs.nix-ld)"

    # Where the assets are: a mirror, or CI's file:// fake release.
    if [ -n "${VINOSPEAK_RELEASE_BASE:-}" ]; then
        BASE="$VINOSPEAK_RELEASE_BASE"
    elif [ -n "$tag" ]; then
        BASE="https://github.com/$REPO/releases/download/$tag"
    else
        BASE="https://github.com/$REPO/releases/latest/download"
    fi
    WORK="$(mktemp -d)"
    # shellcheck disable=SC2064 # expand now
    trap "rm -rf '$WORK'" EXIT

    say "Downloading ${tag:-the latest release} from $BASE"
    curl -fsSL --retry 5 --retry-delay 2 --retry-all-errors --connect-timeout 20 \
        -o "$WORK/SHA256SUMS" "$BASE/SHA256SUMS" || die "download failed: $BASE/SHA256SUMS"
    local wheel
    wheel="$(awk '{ sub(/^[*.\/]+/, "", $2); print $2 }' "$WORK/SHA256SUMS" \
        | grep -E '^vinospeak-[0-9][0-9.]*-py3-none-any\.whl$' | head -1 || true)"
    [ -n "$wheel" ] || die "the release's SHA256SUMS lists no vinospeak wheel"
    fetch "$wheel"
    fetch install.sh
    [ "$gui" = 1 ] && fetch "$GUI_ASSET"
    echo "checksums verified"

    local user_path="$PATH"
    if ! command -v uv >/dev/null && [ ! -x "$BIN/uv" ]; then
        install_uv
    fi
    export PATH="$BIN:$PATH"

    say "Installing ${wheel%-py3-none-any.whl}"
    # --force so an upgrade replaces the tool rather than refusing it. uv's own
    # Python, not /usr/bin's: on Fedora 43+ that's 3.14 too, and an image
    # upgrade to 3.15 would leave the tool's venv pointing at nothing. The env
    # var, not --managed-python, so an older uv already on PATH still runs it.
    UV_PYTHON_PREFERENCE=only-managed uv tool install --force --python "$PYTHON" "$WORK/$wheel"

    if [ "$gui" = 1 ]; then
        say "Installing the window"
        tar -xzf "$WORK/$GUI_ASSET" -C "$WORK"
        install -Dm755 "$WORK/vinospeak-gui/vinospeak-gui" "$BIN/vinospeak-gui"
        mkdir -p "$(dirname "$DESKTOP")" "$(dirname "$ICON")"
        # The entry runs the binary by its path: an app menu's PATH may lack ~/.local/bin.
        EXEC="$BIN/vinospeak-gui" awk '$0 == "Exec=@EXEC@" { print "Exec=" ENVIRON["EXEC"]; next } { print }' \
            "$WORK/vinospeak-gui/dev.vinospeak.Listen.desktop" > "$DESKTOP"
        install -m644 "$WORK/vinospeak-gui/dev.vinospeak.Listen.svg" "$ICON"
        refresh_menu
    fi

    install -Dm755 "$WORK/install.sh" "$BIN/vinospeak-upgrade"
    say "Installed. Try: vinospeak \"Hello.\""
    case ":$user_path:" in *":$BIN:"*) ;; *) echo "Add $BIN to your PATH to run vinospeak by name." ;; esac
}

main "$@"
