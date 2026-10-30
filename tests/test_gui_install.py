"""gui/install.sh writes the app menu entry; cargo and the desktop tools are faked."""

import os
import re
import shutil
import stat
import subprocess
from pathlib import Path

import pytest

SCRIPT = Path(__file__).parent.parent / "gui" / "install.sh"

pytestmark = pytest.mark.skipif(shutil.which("sh") is None, reason="needs a POSIX shell")


def exec_argv(desktop: Path) -> list[str]:
    """The Exec line as a launcher reads it: the file's string escapes are
    undone, then the spec's quoting rules, then %% becomes %."""
    line = next(x for x in desktop.read_text().splitlines() if x.startswith("Exec="))
    escapes = {"s": " ", "n": "\n", "t": "\t", "r": "\r", "\\": "\\"}
    value = re.sub(r"\\(.)", lambda m: escapes[m.group(1)], line.removeprefix("Exec="))
    args: list[str] = []
    arg, quoted, chars = None, False, iter(value)
    for char in chars:
        if quoted and char == "\\":
            arg += next(chars)
        elif char == '"':
            quoted, arg = not quoted, arg or ""
        elif char == " " and not quoted:
            if arg is not None:
                args.append(arg)
            arg = None
        else:
            arg = (arg or "") + char
    if arg is not None:
        args.append(arg)
    return [a.replace("%%", "%") for a in args]


@pytest.fixture
def home(tmp_path):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    # The desktop tools too: update-desktop-database leaves a mimeinfo.cache
    # where it's installed, and that cache is shared, so uninstall keeps it
    for name in ("cargo", "update-desktop-database", "kbuildsycoca6", "kbuildsycoca5"):
        tool = bin_dir / name
        tool.write_text("#!/bin/sh\nexit 0\n")
        tool.chmod(tool.stat().st_mode | stat.S_IXUSR)
    return tmp_path


def run(home: Path, *args: str, vinospeak: Path | None = None) -> None:
    env = {
        "HOME": str(home),
        "XDG_DATA_HOME": str(home / "share"),
        "PATH": f"{home / 'bin'}:/usr/bin:/bin",
    }
    if vinospeak is not None:
        env["VINOSPEAK"] = str(vinospeak)
    subprocess.run(["sh", str(SCRIPT), *args], env=env, check=True, capture_output=True)


def test_install_then_uninstall_leaves_nothing_behind(home):
    run(home)
    desktop = home / "share" / "applications" / "dev.vinospeak.Listen.desktop"
    assert exec_argv(desktop) == [str(home / ".cargo" / "bin" / "vinospeak-gui")]
    assert (home / "share" / "icons" / "hicolor" / "scalable" / "apps").is_dir()

    run(home, "--uninstall")
    assert [p for p in (home / "share").rglob("*") if p.is_file()] == []


def test_a_vinospeak_path_with_awkward_characters_survives(home):
    # Each of these broke the old sed or the unquoted Exec line
    odd = home / 'my code & more|100% "$HOME" `x` \\n' / "vinospeak"
    odd.parent.mkdir()
    odd.touch()
    run(home, vinospeak=odd)
    desktop = home / "share" / "applications" / "dev.vinospeak.Listen.desktop"
    assert exec_argv(desktop) == [
        "env",
        f"VINOSPEAK={odd}",
        str(home / ".cargo" / "bin" / "vinospeak-gui"),
    ]
    if validate := shutil.which("desktop-file-validate"):
        subprocess.run([validate, str(desktop)], check=True)
    assert os.access(desktop, os.R_OK)
