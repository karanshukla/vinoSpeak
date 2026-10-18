"""OpenVINO's bundled telemetry never runs: importing the package blocks it."""

import os
import subprocess
import sys
from pathlib import Path

import pytest

import vino_speak  # noqa: F401 - the import is what blocks it

ROOT = Path(__file__).resolve().parent.parent


def test_importing_vino_speak_blocks_openvino_telemetry() -> None:
    assert "openvino_telemetry" in sys.modules
    assert sys.modules["openvino_telemetry"] is None


def test_real_openvino_writes_nothing_to_intel(tmp_path: Path) -> None:
    """A fresh process with no consent file: no GA client id, no stats."""
    pytest.importorskip("openvino")
    env = {**os.environ, "HOME": str(tmp_path), "https_proxy": "http://127.0.0.1:9"}
    env.pop("CI", None)
    code = "import vino_speak, openvino, openvino_genai; openvino.Core()"
    subprocess.run([sys.executable, "-c", code], cwd=ROOT, env=env, check=True, timeout=120)
    assert not (tmp_path / "intel").exists()
