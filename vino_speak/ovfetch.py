"""NPU diagnosis through ovfetch, the vino apps' version authority.

ovfetch's `detect` reports the OpenVINO range Intel records as working with
this machine's NPU driver. Like vinoWhisper and vinoPop, vinoSpeak only
reports that range: the bounds are pessimistic on purpose ("untested", not
"known to fail"), so nothing here stops the NPU from being tried.

vinoSpeak runs OpenVINO GenAI, not ONNX Runtime, so it never uses
`ovfetch install`: pip provides OpenVINO (pinned in pyproject.toml).
"""

from __future__ import annotations

import json
import re
import shutil
import subprocess
from dataclasses import dataclass
from typing import Any

BINARY = "ovfetch"
INSTALL_HINT = "cargo install ovfetch --locked"
_TIMEOUT_S = 10


@dataclass(frozen=True)
class Note:
    ok: bool | None  # None: nothing to judge by
    label: str
    detail: str


class OvfetchError(Exception):
    pass


def _minor(text: str | None) -> tuple[int, int] | None:
    match = re.match(r"(\d+)\.(\d+)", text or "")
    return (int(match[1]), int(match[2])) if match else None


def _dotted(version: tuple[int, int]) -> str:
    return f"{version[0]}.{version[1]}"


def installed() -> bool:
    return shutil.which(BINARY) is not None


def detect() -> dict[str, Any] | None:
    """`ovfetch detect` as a dict; None when ovfetch is not installed."""
    exe = shutil.which(BINARY)
    if exe is None:
        return None
    try:
        completed = subprocess.run(
            [exe, "detect"], capture_output=True, text=True, timeout=_TIMEOUT_S, check=False
        )
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise OvfetchError(f"{exe} detect: {exc}") from exc
    if completed.returncode != 0:
        tail = (completed.stderr.strip().splitlines() or ["no output"])[-1]
        raise OvfetchError(f"{exe} detect exited {completed.returncode}: {tail}")
    try:
        payload = json.loads(completed.stdout)
    except ValueError as exc:
        raise OvfetchError(f"{exe} detect printed something other than JSON") from exc
    if not isinstance(payload, dict):
        raise OvfetchError(f"{exe} detect printed something other than a JSON object")
    return payload


def notes(status: dict[str, Any], openvino_version: str | None) -> list[Note]:
    """What ovfetch's data says about this machine's NPU and OpenVINO."""
    if "openvino_ceiling" not in status:
        return [Note(None, "ovfetch", "this ovfetch predates platform data in `detect`")]
    if not any(d.get("kind") == "npu" for d in status.get("devices", [])):
        return [Note(True, "ovfetch", "no Intel NPU, so no driver bounds apply")]

    driver = status.get("npu_driver") or "none"
    result = [
        Note(
            True,
            "npu: platform",
            f"{p.get('codename') or 'unnamed'} ({p.get('pci_id')}), "
            f"Intel verifies driver {p.get('min_npu_driver') or '?'} and newer",
        )
        for p in status.get("platforms", [])
    ]
    result += [Note(False, "npu: ovfetch", str(w)) for w in status.get("warnings", [])]

    installed_v = _minor(openvino_version)
    if installed_v is None:
        return result
    ceiling = _minor(status.get("openvino_ceiling"))
    floors = [_minor(p.get("min_openvino")) for p in status.get("platforms", [])]
    floor = max((f for f in floors if f is not None), default=None)
    if ceiling is None:
        result.append(
            Note(
                None,
                "npu: openvino range",
                f"no OpenVINO is recorded as working with NPU driver {driver}",
            )
        )
    elif installed_v > ceiling:
        result.append(
            Note(
                False,
                "npu: openvino range",
                f"OpenVINO {_dotted(installed_v)} is newer than {_dotted(ceiling)}, the newest "
                f"recorded as working with NPU driver {driver}. Untested rather than known "
                "to fail; if NPU compiles fail, suspect this first.",
            )
        )
    elif floor is not None and installed_v < floor:
        result.append(
            Note(
                False,
                "npu: openvino range",
                f"OpenVINO {_dotted(installed_v)} is older than {_dotted(floor)}, the oldest "
                "recorded as working on this platform",
            )
        )
    else:
        low = f"{_dotted(floor)} to " if floor else "up to "
        result.append(
            Note(
                True,
                "npu: openvino range",
                f"OpenVINO {_dotted(installed_v)} is inside {low}{_dotted(ceiling)}, "
                f"recorded as working with NPU driver {driver}",
            )
        )
    return result
