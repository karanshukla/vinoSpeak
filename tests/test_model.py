import hashlib
import json
import re
import sys
from pathlib import Path
from types import ModuleType

import pytest

from vino_speak import model


def make_pins(tmp_path, files):
    """Pins for `files` ({name: bytes}); writes nothing."""
    return model.Pins(
        "org/repo",
        "a" * 40,
        {
            name: {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
            for name, data in files.items()
        },
    )


def write(directory, files):
    for name, data in files.items():
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)


FILES = {name: name.encode() for name in model.MODEL_FILES}


def test_shipped_pins_are_well_formed():
    pins = model.load_pins()
    assert pins.model_id == model.MODEL_ID
    assert re.fullmatch(r"[0-9a-f]{40}", pins.revision)
    assert set(model.MODEL_FILES) <= set(pins.files)
    for name, pin in pins.files.items():
        assert re.fullmatch(r"[0-9a-f]{64}", pin["sha256"]), name
        assert isinstance(pin["size"], int), name


def test_shipped_pins_are_in_the_package_data():
    assert json.loads(model.PINS_PATH.read_text(encoding="utf-8"))["schema"] == 1


def test_default_model_dir_follows_xdg_cache_home(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CACHE_HOME", str(tmp_path))
    assert model.default_model_dir() == tmp_path / "vinospeak" / "kokoro-82m-int8-ov"


def test_model_readiness_requires_all_pipeline_assets(tmp_path):
    assert not model.model_is_ready(tmp_path)
    write(tmp_path, FILES)
    assert model.model_is_ready(tmp_path)


def test_verify_accepts_the_pinned_bytes(tmp_path):
    write(tmp_path, FILES)
    assert model.verify(tmp_path, make_pins(tmp_path, FILES)) == []


def test_verify_names_missing_resized_and_altered_files(tmp_path):
    pins = make_pins(tmp_path, FILES)
    write(
        tmp_path, {**FILES, "config.json": b"longer than pinned", "openvino_model.bin": b"X" * 20}
    )
    (tmp_path / "voices" / "af_heart.bin").unlink()

    problems = model.verify(tmp_path, pins)

    assert any("config.json has" in p for p in problems)
    assert any("voices/af_heart.bin is missing" in p for p in problems)
    assert any("openvino_model.bin" in p for p in problems)


def test_a_same_size_change_needs_the_full_check(tmp_path):
    pins = make_pins(tmp_path, FILES)
    write(tmp_path, {**FILES, "config.json": b"X" * len(FILES["config.json"])})

    assert model.verify(tmp_path, pins, full=False) == []
    assert model.verify(tmp_path, pins) == ["config.json does not match its pinned sha256"]


@pytest.fixture
def fake_hub(monkeypatch, tmp_path):
    pins = make_pins(tmp_path, FILES)
    monkeypatch.setattr(model, "load_pins", lambda: pins)
    calls = []
    hub = ModuleType("huggingface_hub")

    def snapshot_download(**kwargs):
        calls.append(kwargs)
        write(Path(kwargs["local_dir"]), hub.contents)

    hub.snapshot_download = snapshot_download
    hub.contents = FILES
    hub.calls = calls
    monkeypatch.setitem(sys.modules, "huggingface_hub", hub)
    return hub


def test_ensure_model_downloads_the_pinned_revision_when_missing(fake_hub, tmp_path):
    target = tmp_path / "kokoro"

    assert model.ensure_model(target) == target

    (call,) = fake_hub.calls
    assert call["repo_id"] == "org/repo"
    assert call["revision"] == "a" * 40
    assert sorted(call["allow_patterns"]) == sorted(FILES)


def test_ensure_model_rejects_a_download_that_does_not_match(fake_hub, tmp_path):
    fake_hub.contents = {**FILES, "config.json": b"tampered!"}
    with pytest.raises(model.ModelIntegrityError, match="failed verification"):
        model.ensure_model(tmp_path / "kokoro")


def test_ensure_model_does_not_download_a_model_already_there(fake_hub, tmp_path):
    write(tmp_path, FILES)
    model.ensure_model(tmp_path)
    assert fake_hub.calls == []


def test_ensure_model_refuses_a_stale_cache(fake_hub, tmp_path):
    write(tmp_path, {**FILES, "openvino_model.bin": b"an older, longer export"})
    with pytest.raises(model.ModelIntegrityError, match="not the pinned revision"):
        model.ensure_model(tmp_path)
    assert fake_hub.calls == []


def test_a_directory_you_supply_is_used_as_given(fake_hub, tmp_path):
    write(tmp_path, {name: b"your own export" for name in model.MODEL_FILES})
    assert model.ensure_model(tmp_path, trusted=True) == tmp_path
    assert fake_hub.calls == []


def test_an_empty_directory_you_supply_still_gets_the_pinned_model(fake_hub, tmp_path):
    model.ensure_model(tmp_path / "mine", trusted=True)
    assert len(fake_hub.calls) == 1


def test_a_failed_download_is_not_used_on_the_next_run(fake_hub, tmp_path):
    # Right size, wrong bytes: the size check alone would pass it.
    bad = b"X" * len(FILES["config.json"])
    fake_hub.contents = {**FILES, "config.json": bad}
    target = tmp_path / "kokoro"
    with pytest.raises(model.ModelIntegrityError, match="removed"):
        model.ensure_model(target)
    assert not (target / "config.json").exists()

    # Run again, as a user would: it downloads again instead of loading.
    fake_hub.contents = FILES
    assert model.ensure_model(target) == target
    assert len(fake_hub.calls) == 2
    assert (target / "config.json").read_bytes() == FILES["config.json"]


def test_an_unstamped_model_is_hashed_in_full(fake_hub, tmp_path):
    # Left by an older version, or a download interrupted before its check.
    write(tmp_path, {**FILES, "config.json": b"X" * len(FILES["config.json"])})
    with pytest.raises(model.ModelIntegrityError, match="not the pinned revision"):
        model.ensure_model(tmp_path)
    assert not (tmp_path / model.STAMP).exists()


def test_a_verified_model_is_stamped_then_size_checked(fake_hub, tmp_path, monkeypatch):
    write(tmp_path, FILES)
    model.ensure_model(tmp_path)
    assert (tmp_path / model.STAMP).is_file()

    hashed = []
    real = model.sha256_file
    monkeypatch.setattr(model, "sha256_file", lambda p: hashed.append(p) or real(p))
    model.ensure_model(tmp_path)
    assert hashed == []


def test_new_pins_invalidate_the_stamp(fake_hub, tmp_path):
    write(tmp_path, FILES)
    model.ensure_model(tmp_path)
    pins = make_pins(tmp_path, FILES)
    assert model._is_stamped(tmp_path, pins)
    assert not model._is_stamped(tmp_path, model.Pins(pins.model_id, "b" * 40, pins.files))


def test_a_download_without_huggingface_hub_says_what_is_missing(fake_hub, tmp_path, monkeypatch):
    monkeypatch.setitem(sys.modules, "huggingface_hub", None)
    with pytest.raises(RuntimeError, match="huggingface-hub is required"):
        model.ensure_model(tmp_path / "kokoro")
    assert not (tmp_path / "kokoro").exists()
