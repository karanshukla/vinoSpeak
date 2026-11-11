"""install.sh --uninstall, run for real against a scratch home; uv is faked."""

import shutil
import stat
import subprocess
from pathlib import Path

import pytest

SCRIPT = Path(__file__).parent.parent / "install.sh"

pytestmark = pytest.mark.skipif(shutil.which("bash") is None, reason="needs bash")


def tree(root: Path, *names: str) -> None:
    for name in names:
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(name)


def uninstall(home: Path, **env: str) -> str:
    bin_dir = home / ".local" / "bin"
    uv = bin_dir / "uv"
    uv.write_text(f'#!/bin/sh\necho "$*" >> {home}/uv-calls\n')
    uv.chmod(uv.stat().st_mode | stat.S_IXUSR)
    result = subprocess.run(
        ["bash", str(SCRIPT), "--uninstall"],
        env={"HOME": str(home), "PATH": f"{bin_dir}:/usr/bin:/bin", **env},
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


@pytest.fixture
def home(tmp_path):
    tree(
        tmp_path,
        ".local/bin/vinospeak-gui",
        ".local/bin/vinospeak-upgrade",
        ".local/share/applications/dev.vinospeak.Listen.desktop",
        ".local/share/applications/someone-else.desktop",
    )
    return tmp_path


def test_uninstall_deletes_the_model_and_the_compile_cache(home):
    tree(
        home,
        ".cache/vinospeak/kokoro-82m-int8-ov/openvino_model.bin",
        ".cache/vinospeak/ov-cache/blob",
        ".cache/huggingface/hub/shared",
        ".cache/vinowhisper/keep",
    )
    out = uninstall(home)
    assert not (home / ".cache" / "vinospeak").exists()
    assert (home / ".cache" / "huggingface" / "hub" / "shared").is_file()
    assert (home / ".cache" / "vinowhisper" / "keep").is_file()
    assert not (home / ".local" / "bin" / "vinospeak-gui").exists()
    assert not (home / ".local" / "bin" / "vinospeak-upgrade").exists()
    assert (home / ".local" / "share" / "applications" / "someone-else.desktop").is_file()
    assert (home / "uv-calls").read_text() == "tool uninstall vinospeak\n"
    assert f"{home}/.cache/vinospeak" in out


def test_uninstall_follows_xdg_cache_home(home):
    tree(home, "elsewhere/vinospeak/kokoro-82m-int8-ov/config.json", ".cache/vinospeak/kept")
    out = uninstall(home, XDG_CACHE_HOME=str(home / "elsewhere"))
    assert not (home / "elsewhere" / "vinospeak").exists()
    assert (home / ".cache" / "vinospeak" / "kept").is_file()
    assert f"{home}/elsewhere/vinospeak" in out


def test_uninstall_without_a_model_is_fine(home):
    assert "removed" in uninstall(home)
