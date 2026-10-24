import sys
import threading

import numpy as np
import pytest

from vino_speak import playback
from vino_speak.tts import Audio

# The real probe, before the fixture below stands in for it
streams_raw = playback.streams_raw


@pytest.fixture(autouse=True)
def every_player_streams_raw(monkeypatch):
    monkeypatch.setattr(playback, "streams_raw", lambda player: True)


def test_pipewire_is_preferred_over_pulseaudio_and_alsa(monkeypatch):
    monkeypatch.setattr(playback.shutil, "which", lambda name: f"/usr/bin/{name}")
    assert playback.find_player() == "/usr/bin/pw-play"


def test_an_old_pw_play_is_skipped_for_paplay(monkeypatch):
    monkeypatch.setattr(playback.shutil, "which", lambda name: f"/usr/bin/{name}")
    monkeypatch.setattr(playback, "streams_raw", lambda player: not player.endswith("pw-play"))
    assert playback.find_player() == "/usr/bin/paplay"


def fake_help(monkeypatch, help_text=None, error=None):
    calls = []

    def run(command, **kwargs):
        calls.append(command)
        if error is not None:
            raise error
        return playback.subprocess.CompletedProcess(command, 0, help_text, "")

    monkeypatch.setattr(playback.subprocess, "run", run)
    streams_raw.cache_clear()
    return calls


@pytest.mark.parametrize(
    ("help_text", "expected"),
    [
        ("  -a, --raw                             RAW mode\n", True),
        ("      --rate                            Sample rate\n", False),
    ],
)
def test_pw_play_is_asked_whether_it_has_raw(monkeypatch, help_text, expected):
    calls = fake_help(monkeypatch, help_text)
    assert streams_raw("/usr/bin/pw-play") is expected
    assert streams_raw("/usr/bin/pw-play") is expected
    assert calls == [["/usr/bin/pw-play", "--help"]]
    streams_raw.cache_clear()


def test_a_pw_play_that_will_not_run_is_skipped(monkeypatch):
    fake_help(monkeypatch, error=OSError("exec format error"))
    assert streams_raw("/usr/bin/pw-play") is False
    streams_raw.cache_clear()


def test_only_pw_play_is_probed(monkeypatch):
    calls = fake_help(monkeypatch, "")
    assert streams_raw("/usr/bin/paplay")
    assert streams_raw("/usr/bin/aplay")
    assert calls == []
    streams_raw.cache_clear()


def test_falls_back_to_alsa(monkeypatch):
    monkeypatch.setattr(
        playback.shutil, "which", lambda name: "/usr/bin/aplay" if name == "aplay" else None
    )
    assert playback.find_player() == "/usr/bin/aplay"


def test_no_player_says_how_to_save_instead(monkeypatch):
    monkeypatch.setattr(playback.shutil, "which", lambda name: None)
    with pytest.raises(RuntimeError, match="--output"):
        playback.require_player()


@pytest.mark.parametrize("player", playback.PLAYERS)
def test_every_player_is_fed_mono_float32_at_the_models_rate(player):
    command = playback.raw_command(f"/usr/bin/{player}", 24000)
    assert command[0] == f"/usr/bin/{player}"
    assert "24000" in " ".join(command)


@pytest.mark.parametrize(
    ("player", "flags"),
    [
        ("pw-play", ["--target=hdmi_sink"]),
        ("paplay", ["--device=hdmi_sink"]),
        ("aplay", ["-D", "hdmi_sink"]),
    ],
)
def test_an_audio_device_is_passed_to_the_player(player, flags):
    command = playback.raw_command(f"/usr/bin/{player}", 24000, "hdmi_sink")
    assert all(flag in command for flag in flags)
    assert "hdmi_sink" not in " ".join(playback.raw_command(f"/usr/bin/{player}", 24000))


def test_an_unknown_player_is_refused():
    with pytest.raises(ValueError, match="mpv"):
        playback.raw_command("/usr/bin/mpv", 24000)


def player_running(monkeypatch, script):
    monkeypatch.setattr(
        playback, "raw_command", lambda player, rate, device=None: [sys.executable, "-c", script]
    )


def chunk(*values):
    return Audio(np.asarray(values, dtype=np.float32), 24000)


def test_chunks_reach_the_player_in_order(monkeypatch, tmp_path):
    out = tmp_path / "played.raw"
    player_running(
        monkeypatch, f"import sys; open({str(out)!r}, 'wb').write(sys.stdin.buffer.read())"
    )
    speech = playback.Speech([chunk(0.1, 0.2), chunk(0.3)], player="fake")
    assert speech.wait(timeout=10)
    assert speech.done
    np.testing.assert_allclose(np.fromfile(out, dtype="<f4"), [0.1, 0.2, 0.3])


def test_stop_cuts_playback_off_without_waiting_for_synthesis(monkeypatch):
    player_running(monkeypatch, "import time; time.sleep(30)")
    release = threading.Event()

    def slow_synthesis():
        yield chunk(0.1)
        release.wait(30)
        yield chunk(0.2)

    speech = playback.Speech(slow_synthesis(), player="fake")
    speech.stop()
    try:
        assert speech.wait(timeout=5)
    finally:
        release.set()


def test_a_synthesis_error_is_raised_from_wait(monkeypatch):
    player_running(monkeypatch, "import sys; sys.stdin.buffer.read()")

    def failing_synthesis():
        yield chunk(0.1)
        raise RuntimeError("NPU went away")

    with pytest.raises(RuntimeError, match="NPU went away"):
        playback.Speech(failing_synthesis(), player="fake").wait(timeout=10)


def test_a_failing_player_is_raised_from_wait(monkeypatch):
    player_running(monkeypatch, "import sys; sys.stdin.buffer.read(); sys.exit(3)")
    with pytest.raises(RuntimeError, match="exited with status 3"):
        playback.Speech([chunk(0.1)], player="fake").wait(timeout=10)


def test_nothing_to_say_never_starts_a_player(monkeypatch):
    monkeypatch.setattr(playback, "raw_command", lambda *args: pytest.fail("player started"))
    assert playback.Speech([], player="fake").wait(timeout=5)


def test_a_dead_player_releases_the_synthesis_thread(monkeypatch):
    player_running(monkeypatch, "import sys; sys.exit(3)")

    def endless_synthesis():
        while True:
            yield chunk(0.1)

    speech = playback.Speech(endless_synthesis(), player="fake")
    with pytest.raises(RuntimeError, match="exited with status 3"):
        speech.wait(timeout=10)
    speech._synthesis.join(timeout=5)
    assert not speech._synthesis.is_alive()
