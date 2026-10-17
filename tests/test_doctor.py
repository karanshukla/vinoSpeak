import json

import pytest

from vino_speak import doctor, ovfetch
from vino_speak.model import MODEL_FILES


@pytest.fixture(autouse=True)
def isolated(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CACHE_HOME", str(tmp_path))
    monkeypatch.setattr(doctor, "_distribution", lambda name: "1.0")
    monkeypatch.setattr(
        doctor, "_devices", lambda: [doctor.Result(doctor.OK, "devices", "CPU, NPU")]
    )


def by_label(results, label):
    return next(r for r in results if r.label == label)


def test_a_missing_package_is_a_failure(monkeypatch):
    monkeypatch.setattr(doctor, "_distribution", lambda name: None)
    assert by_label(doctor._packages(), "openvino-genai").status == doctor.FAIL


def test_a_model_not_yet_downloaded_is_unknown_not_failed():
    assert by_label(doctor._model(), "model").status == doctor.UNKNOWN


def test_a_stale_model_fails(monkeypatch, tmp_path):
    for name in MODEL_FILES:
        path = tmp_path / "vinospeak" / "kokoro-82m-int8-ov" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"stale")
    assert by_label(doctor._model(), "model").status == doctor.FAIL


def test_ovfetch_is_only_consulted_with_an_npu(monkeypatch):
    monkeypatch.setattr(ovfetch, "detect", lambda: pytest.fail("no NPU, so no ovfetch"))
    assert doctor._ovfetch(has_npu=False) == []


def test_a_missing_ovfetch_is_unknown_with_an_install_hint(monkeypatch):
    monkeypatch.setattr(ovfetch, "detect", lambda: None)
    (result,) = doctor._ovfetch(has_npu=True)
    assert result.status == doctor.UNKNOWN
    assert ovfetch.INSTALL_HINT in result.detail


def test_a_broken_ovfetch_is_a_warning_never_a_failure(monkeypatch):
    def broken():
        raise ovfetch.OvfetchError("ovfetch detect exited 2: boom")

    monkeypatch.setattr(ovfetch, "detect", broken)
    (result,) = doctor._ovfetch(has_npu=True)
    assert result.status == doctor.WARN


def test_ovfetch_notes_become_results_and_never_fail(monkeypatch):
    status = {
        "devices": [{"kind": "npu"}],
        "openvino_ceiling": "2025.4",
        "platforms": [],
        "npu_driver": "1.32.0",
    }
    monkeypatch.setattr(ovfetch, "detect", lambda: status)
    monkeypatch.setattr(doctor, "_distribution", lambda name: "2026.4.1")
    results = doctor._ovfetch(has_npu=True)
    assert by_label(results, "npu: openvino range").status == doctor.WARN
    assert all(r.status != doctor.FAIL for r in results)


def test_json_output_and_exit_status(capsys, monkeypatch):
    monkeypatch.setattr(doctor, "_ovfetch", lambda has_npu: [])
    code = doctor.main(["--json"])
    report = json.loads(capsys.readouterr().out)
    assert code == 0
    assert report["failed"] == []
    assert by_label([doctor.Result(**c) for c in report["checks"]], "vinospeak").status == doctor.OK


def test_exit_status_is_1_when_something_failed(capsys, monkeypatch):
    monkeypatch.setattr(doctor, "_distribution", lambda name: None)
    assert doctor.main([]) == 1
    assert "failed" in capsys.readouterr().out
