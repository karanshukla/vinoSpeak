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
