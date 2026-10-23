#!/bin/sh
# Install the vinoSpeak window for this user: the binary into ~/.cargo/bin,
# its app menu entry and icon into ~/.local/share. --uninstall removes them.
#
# VINOSPEAK=/path/to/vinospeak bakes a vinospeak into the menu entry (a
# checkout's .venv/bin/vinospeak, until a release has --file); otherwise the
# window finds vinospeak on PATH or in ~/.local/bin.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
desktop=$data/applications/dev.vinospeak.Listen.desktop
icon=$data/icons/hicolor/scalable/apps/dev.vinospeak.Listen.svg

refresh() {
    update-desktop-database -q "$data/applications" 2>/dev/null || true
    kbuildsycoca6 >/dev/null 2>&1 || kbuildsycoca5 >/dev/null 2>&1 || true
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$desktop" "$icon"
    cargo uninstall vinospeak-gui 2>/dev/null || true
    refresh
    echo "vinoSpeak window removed"
    exit 0
fi

cargo install --quiet --path "$here"
bin=$(command -v vinospeak-gui || echo "$HOME/.cargo/bin/vinospeak-gui")
exec_line=$bin
if [ -n "${VINOSPEAK:-}" ]; then
    exec_line="env VINOSPEAK=$(realpath "$VINOSPEAK") $bin"
fi

mkdir -p "$(dirname "$desktop")" "$(dirname "$icon")"
sed "s|@EXEC@|$exec_line|" "$here/data/dev.vinospeak.Listen.desktop" > "$desktop"
cp "$here/../docs/assets/vinospeak.svg" "$icon"
refresh
echo "vinoSpeak is in the app menu (runs $exec_line)"
