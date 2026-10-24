"""NPU diagnosis through ovfetch, the vino apps' version authority.

ovfetch's `detect` reports the OpenVINO range Intel records as working with
this machine's NPU driver. Like vinoWhisper and vinoPop, vinoSpeak only
reports that range: the bounds are pessimistic on purpose ("untested", not
"known to fail"), so nothing here stops the NPU from being tried.

vinoSpeak runs OpenVINO GenAI, not ONNX Runtime, so it never uses
`ovfetch install`: pip provides OpenVINO (pinned in pyproject.toml). The one
thing pip cannot provide is the NPU compiler a distro's driver package may
lack (Fedora's 1.32.0 rpm): `ovfetch npu install` (0.6.0) puts it in
~/.local/lib/ovfetch-npu, and preload() loads it from there.
"""

from __future__ import annotations

import ctypes
import json
import re
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any

BINARY = "ovfetch"
INSTALL_HINT = "cargo install ovfetch --locked"
_TIMEOUT_S = 10
# Where `ovfetch npu install` puts the compiler (or a whole driver), and the
# list it writes there: the libraries to load, one per line, in order.
NPU_PREFIX = Path.home() / ".local/lib/ovfetch-npu"
PRELOAD_LIST = "ovfetch-preload.txt"


@dataclass(frozen=True)
class Note:
    ok: bool | None  # None: nothing to judge by
    label: str
    detail: str


class OvfetchError(Exception):
    pass


def npu_prefix() -> Path | None:
    """The default prefix, once `ovfetch npu install` has written its list."""
    try:
        return NPU_PREFIX if (NPU_PREFIX / PRELOAD_LIST).is_file() else None
    except OSError:
        return None


# What preload() loaded, once it has: a failed attempt is retried next time.
_preloaded: list[str] | None = None


def preload(prefix: Path | None = None) -> list[str]:
    """Load the prefix's libraries so the NPU driver finds its compiler (or so
    the prefix's driver is the one that runs). Each line of the list in its
    order with RTLD_GLOBAL: the driver later asks for them by soname and gets
    the copies already loaded. After `import openvino` (the compiler links
    its TBB), before anything opens the NPU; a second call is a no-op. No
    prefix, no list: nothing happens. Returns what was loaded."""
    global _preloaded
    if _preloaded is not None:
        return list(_preloaded)
    prefix = prefix if prefix is not None else npu_prefix()
    if prefix is None:
        _preloaded = []
        return []
    listing = prefix / PRELOAD_LIST
    try:
        names = [line.strip() for line in listing.read_text(encoding="utf-8").splitlines()]
    except FileNotFoundError:
        _preloaded = []
        return []
    except OSError as exc:
        raise OvfetchError(f"{listing} is unreadable: {exc}") from exc
    loaded: list[str] = []
    for name in filter(None, names):
        try:
            ctypes.CDLL(name, mode=ctypes.RTLD_GLOBAL)
        except OSError as exc:
            raise OvfetchError(f"loading {name} (listed in {listing}): {exc}") from exc
        loaded.append(name)
    _preloaded = loaded
    return list(loaded)


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
    if status.get("npu_prefix"):
        result.append(
            Note(
                True,
                "npu: ovfetch prefix",
                f"{status['npu_prefix']} (ovfetch npu install); vinoSpeak loads its "
                "libraries before OpenVINO opens the NPU",
            )
        )

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
