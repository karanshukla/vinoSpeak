"""ovfetch's `detect` JSON, read into doctor notes.

The fixture is real `ovfetch detect` output from the Wildcat Lake laptop
(2026-09-28, as captured in vinoWhisper's tests). Nothing here runs ovfetch:
`subprocess.run` and `shutil.which` are replaced.
"""

import copy
import json
import subprocess

import pytest

from vino_speak import ovfetch

WILDCAT_LAKE = {
    "devices": [
        {"kind": "npu", "pci_id": "0xfd3e", "slot": "0000:00:0b.0"},
        {"kind": "gpu", "pci_id": "0xfd80", "slot": "0000:00:02.0"},
    ],
    "npu_driver": "1.35.0",
    "npu_compiler": True,
    "platforms": [
        {
            "pci_id": "0xfd3e",
            "kernel_name": "WCL",
            "codename": "Wildcat Lake",
            "min_openvino": "2025.4",
            "min_npu_driver": "1.32.0",
        }
    ],
    "openvino_ceiling": "2026.4",
    "npu_driver_required": "1.32.0",
    "warnings": [],
}


def status(**changes):
    result = copy.deepcopy(WILDCAT_LAKE)
    result.update(changes)
    return result


def labelled(notes, label):
    return next(note for note in notes if note.label == label)


def test_the_laptops_own_setup_is_inside_the_range():
    notes = ovfetch.notes(status(), "2026.4.1")
    assert all(note.ok is True for note in notes)
    assert "Wildcat Lake" in labelled(notes, "npu: platform").detail
    assert "2025.4 to 2026.4" in labelled(notes, "npu: openvino range").detail


def test_newer_than_the_ceiling_is_a_warning_not_a_failure():
    note = labelled(
        ovfetch.notes(status(openvino_ceiling="2026.3"), "2026.4.1"), "npu: openvino range"
    )
    assert note.ok is False
    assert "Untested rather than known to fail" in note.detail


def test_older_than_the_floor_is_a_warning():
    note = labelled(ovfetch.notes(status(), "2025.2.0"), "npu: openvino range")
    assert note.ok is False
    assert "older than 2025.4" in note.detail


def test_no_recorded_ceiling_is_unknown_not_bad():
    note = labelled(ovfetch.notes(status(openvino_ceiling=None), "2026.4.1"), "npu: openvino range")
    assert note.ok is None


def test_ovfetchs_own_warnings_are_passed_on():
    notes = ovfetch.notes(status(warnings=["no NPU compiler"]), None)
    assert labelled(notes, "npu: ovfetch").detail == "no NPU compiler"
    assert not any(note.label == "npu: openvino range" for note in notes)


def test_no_npu_means_no_bounds():
    (note,) = ovfetch.notes(status(devices=[{"kind": "gpu"}]), "2026.4.1")
    assert note.ok is True
    assert "no Intel NPU" in note.detail


def test_an_old_ovfetch_without_platform_data_is_unknown():
    (note,) = ovfetch.notes({"devices": []}, "2026.4.1")
    assert note.ok is None


def test_a_prefix_from_npu_install_is_reported():
    """ovfetch 0.6.0 names the prefix whose compiler or driver it counted."""
    note = labelled(
        ovfetch.notes(status(npu_prefix="/home/k/.local/lib/ovfetch-npu"), "2026.4.1"),
        "npu: ovfetch prefix",
    )
    assert note.ok is True
    assert "/home/k/.local/lib/ovfetch-npu" in note.detail
    assert all(n.label != "npu: ovfetch prefix" for n in ovfetch.notes(status(), "2026.4.1"))


# --- the NPU prefix's libraries --------------------------------------------
#
# `ovfetch npu install` puts the compiler a distro's driver package lacks (or
# a whole driver) in ~/.local/lib/ovfetch-npu, off the linker path. vinoSpeak
# loads them itself, in the order ovfetch-preload.txt gives, before OpenVINO
# opens the NPU. The order is the file's: a driver loaded before the Level
# Zero loader crashed (ovfetch's README, measured 2026-10-24).


@pytest.fixture
def loader(monkeypatch):
    loaded: list[tuple[str, int]] = []
    monkeypatch.setattr(ovfetch, "_preloaded", None)
    monkeypatch.setattr(
        ovfetch.ctypes, "CDLL", lambda name, mode=0: loaded.append((name, mode)) or object()
    )
    return loaded


def prefix_with(tmp_path, *lines):
    prefix = tmp_path / "ovfetch-npu"
    prefix.mkdir()
    (prefix / ovfetch.PRELOAD_LIST).write_text("\n".join(lines) + "\n", encoding="utf-8")
    return prefix


def test_preload_loads_the_list_in_its_order_and_globally(tmp_path, loader):
    prefix = prefix_with(tmp_path, "libze_loader.so.1", f"{tmp_path}/libze_intel_npu.so.1", "")
    assert ovfetch.preload(prefix) == ["libze_loader.so.1", f"{tmp_path}/libze_intel_npu.so.1"]
    assert [name for name, _ in loader] == ovfetch.preload(prefix)
    assert all(mode == ovfetch.ctypes.RTLD_GLOBAL for _, mode in loader)


def test_preload_runs_once(tmp_path, loader):
    prefix = prefix_with(tmp_path, "libnpu_driver_compiler.so")
    ovfetch.preload(prefix)
    ovfetch.preload(prefix)
    assert len(loader) == 1


def test_preload_does_nothing_without_a_prefix(tmp_path, loader, monkeypatch):
    monkeypatch.setattr(ovfetch, "NPU_PREFIX", tmp_path / "absent")
    assert ovfetch.npu_prefix() is None
    assert ovfetch.preload() == []
    assert loader == []


def test_the_default_prefix_counts_once_it_holds_the_list(tmp_path, monkeypatch):
    monkeypatch.setattr(ovfetch, "NPU_PREFIX", tmp_path / "ovfetch-npu")
    (tmp_path / "ovfetch-npu").mkdir()
    assert ovfetch.npu_prefix() is None
    (tmp_path / "ovfetch-npu" / ovfetch.PRELOAD_LIST).write_text("x\n", encoding="utf-8")
    assert ovfetch.npu_prefix() == tmp_path / "ovfetch-npu"


def test_a_library_that_will_not_load_is_named_and_retried(tmp_path, loader, monkeypatch):
    prefix = prefix_with(tmp_path, "libmissing.so")

    def failing(name, mode=0):
        raise OSError(f"{name}: cannot open shared object file")

    monkeypatch.setattr(ovfetch.ctypes, "CDLL", failing)
    with pytest.raises(ovfetch.OvfetchError, match="libmissing.so"):
        ovfetch.preload(prefix)
    assert ovfetch._preloaded is None


def run_detect(monkeypatch, completed):
    monkeypatch.setattr(ovfetch.shutil, "which", lambda name: f"/usr/bin/{name}")
    monkeypatch.setattr(ovfetch.subprocess, "run", lambda *args, **kwargs: completed)


def completed(stdout="", returncode=0, stderr=""):
    return subprocess.CompletedProcess(["ovfetch", "detect"], returncode, stdout, stderr)


def test_detect_without_ovfetch_is_none(monkeypatch):
    monkeypatch.setattr(ovfetch.shutil, "which", lambda name: None)
    assert not ovfetch.installed()
    assert ovfetch.detect() is None


def test_detect_parses_the_json(monkeypatch):
    run_detect(monkeypatch, completed(json.dumps(WILDCAT_LAKE)))
    assert ovfetch.detect() == WILDCAT_LAKE


@pytest.mark.parametrize(
    "bad",
    [
        completed("", returncode=2, stderr="boom\nlast line"),
        completed("not json"),
        completed("[1, 2]"),
    ],
)
def test_detect_reports_unusable_output(monkeypatch, bad):
    run_detect(monkeypatch, bad)
    with pytest.raises(ovfetch.OvfetchError):
        ovfetch.detect()


def test_detect_reports_a_timeout(monkeypatch):
    monkeypatch.setattr(ovfetch.shutil, "which", lambda name: "/usr/bin/ovfetch")

    def hang(*args, **kwargs):
        raise subprocess.TimeoutExpired("ovfetch", 10)

    monkeypatch.setattr(ovfetch.subprocess, "run", hang)
    with pytest.raises(ovfetch.OvfetchError, match="detect"):
        ovfetch.detect()
