"""`vinospeak-doctor`: what text-to-speech needs from this machine.

Modelled on vinowhisper-doctor and `vinopop --doctor`. Reads, never changes
anything, and works over SSH and in a bug report (`--json`).
"""

from __future__ import annotations

import argparse
import importlib.metadata
import json
import shutil
import sys
from collections.abc import Sequence
from dataclasses import asdict, dataclass

from . import __version__, ovfetch, playback
from .model import default_model_dir, model_is_ready, verify

OK, WARN, FAIL, UNKNOWN = "ok", "warn", "FAIL", "??"


@dataclass
class Result:
    status: str
    label: str
    detail: str = ""


def _distribution(name: str) -> str | None:
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None


def _packages() -> list[Result]:
    results = []
    for dist, needed_for in (
        ("openvino", "every device"),
        ("openvino-genai", "the Kokoro pipeline"),
        ("espeakng-loader", "phonemes"),
        ("soundfile", "writing WAV files"),
        ("huggingface-hub", "the one-time model download"),
    ):
        version = _distribution(dist)
        if version is None:
            results.append(Result(FAIL, dist, f"missing, needed for {needed_for}"))
        else:
            results.append(Result(OK, dist, version))
    return results


def _devices() -> list[Result]:
    try:
        import openvino as ov

        found = list(ov.Core().available_devices)
    except Exception as exc:  # noqa: BLE001 - reported, not raised
        return [Result(FAIL, "devices", f"OpenVINO did not start: {exc}")]
    results = [Result(OK, "devices", ", ".join(found) or "none")]
    if not any(d.startswith("NPU") for d in found):
        results.append(
            Result(WARN, "npu", "not enumerated by OpenVINO; pass --device CPU or --device GPU")
        )
    return results


def _ovfetch(has_npu: bool) -> list[Result]:
    if not has_npu:
        return []
    try:
        status = ovfetch.detect()
    except ovfetch.OvfetchError as exc:
        return [Result(WARN, "ovfetch", str(exc))]
    if status is None:
        return [
            Result(
                UNKNOWN,
                "ovfetch",
                f"not installed; `{ovfetch.INSTALL_HINT}` adds Intel's per-platform NPU driver data",
            )
        ]
    # Its bounds are pessimistic on purpose, so nothing it says is a FAIL.
    return [
        Result(OK if n.ok else (UNKNOWN if n.ok is None else WARN), n.label, n.detail)
        for n in ovfetch.notes(status, _distribution("openvino"))
    ]


def _playback() -> list[Result]:
    player = playback.find_player()
    if player is None:
        return [
            Result(
                WARN,
                "playback",
                f"no {', '.join(playback.PLAYERS)} that can stream; only --output works",
            )
        ]
    detail = player
    old = shutil.which("pw-play")
    if old is not None and old != player:
        detail += " (this pw-play is too old to stream raw audio)"
    return [Result(OK, "playback", detail)]


def _model() -> list[Result]:
    model_dir = default_model_dir()
    if not model_is_ready(model_dir):
        return [
            Result(
                UNKNOWN,
                "model",
                f"not downloaded yet; the first run fetches ~160 MB into {model_dir}",
            )
        ]
    problems = verify(model_dir)
    if problems:
        return [Result(FAIL, "model", f"{model_dir}: {'; '.join(problems[:3])}")]
    return [Result(OK, "model", f"{model_dir}, every file matches its pinned sha256")]


def collect() -> list[Result]:
    py = sys.version_info
    results = [
        Result(OK, "vinospeak", __version__),
        Result(OK, "python", f"{py.major}.{py.minor}.{py.micro}"),
    ]
    results += _packages()
    devices = _devices() if _distribution("openvino") else []
    results += devices
    has_npu = any(r.label == "devices" and "NPU" in r.detail for r in devices)
    results += _ovfetch(has_npu)
    results += _playback()
    results += _model()
    return results


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Check what vinoSpeak needs from this machine.")
    parser.add_argument("--json", action="store_true", help="machine-readable output")
    arguments = parser.parse_args(argv)

    results = collect()
    failed = [r.label for r in results if r.status == FAIL]
    if arguments.json:
        print(json.dumps({"checks": [asdict(r) for r in results], "failed": failed}, indent=2))
        return 1 if failed else 0
    width = max(len(r.label) for r in results)
    for r in results:
        print(f"  [{r.status:>4}] {r.label:<{width}}  {r.detail}")
    tally = {s: sum(r.status == s for r in results) for s in (OK, WARN, UNKNOWN)}
    print(f"\n{tally[OK]} ok, {tally[WARN]} warn, {tally[UNKNOWN]} unknown, {len(failed)} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
