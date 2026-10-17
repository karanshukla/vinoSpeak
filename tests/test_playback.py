import subprocess

import pytest

from vino_speak import playback


def test_pipewire_is_preferred_over_pulseaudio_and_alsa(monkeypatch):
    monkeypatch.setattr(playback.shutil, "which", lambda name: f"/usr/bin/{name}")
    assert playback.find_player() == "/usr/bin/pw-play"


def test_falls_back_to_alsa(monkeypatch):
    monkeypatch.setattr(
        playback.shutil, "which", lambda name: "/usr/bin/aplay" if name == "aplay" else None
    )
    assert playback.find_player() == "/usr/bin/aplay"


def test_no_player_says_how_to_save_instead(monkeypatch, tmp_path):
    monkeypatch.setattr(playback.shutil, "which", lambda name: None)
    with pytest.raises(RuntimeError, match="--output"):
        playback.play(tmp_path / "speech.wav")


def test_a_failing_player_is_a_runtime_error(monkeypatch, tmp_path):
    monkeypatch.setattr(playback, "find_player", lambda: "/usr/bin/pw-play")

    def fail(command, check):
        raise subprocess.CalledProcessError(1, command)

    monkeypatch.setattr(playback.subprocess, "run", fail)
    with pytest.raises(RuntimeError, match="pw-play exited with status 1"):
        playback.play(tmp_path / "speech.wav")
