"""Play speech through the desktop's sound server while it is still being
synthesized."""

from __future__ import annotations

import contextlib
import functools
import queue
import shutil
import subprocess
import threading
from collections.abc import Callable, Iterable, Iterator
from pathlib import Path
from typing import IO, TYPE_CHECKING

if TYPE_CHECKING:
    from .tts import Audio


def _pw_play(player: str, sample_rate: int, audio_device: str | None) -> list[str]:
    target = [f"--target={audio_device}"] if audio_device else []
    return [player, "--raw", "--format=f32", "--channels=1", f"--rate={sample_rate}", *target, "-"]


def _paplay(player: str, sample_rate: int, audio_device: str | None) -> list[str]:
    target = [f"--device={audio_device}"] if audio_device else []
    return [player, "--raw", "--format=float32le", "--channels=1", f"--rate={sample_rate}", *target]


def _aplay(player: str, sample_rate: int, audio_device: str | None) -> list[str]:
    target = ["-D", audio_device] if audio_device else []
    return [player, "-q", "-t", "raw", "-f", "FLOAT_LE", "-c", "1", "-r", str(sample_rate), *target]


# In order of preference (pinned by
# `test_playback.py::test_pipewire_is_preferred_over_pulseaudio_and_alsa`).
RAW_COMMANDS = {"pw-play": _pw_play, "paplay": _paplay, "aplay": _aplay}
PLAYERS = tuple(RAW_COMMANDS)


@functools.cache
def streams_raw(player: str) -> bool:
    """Whether this player can take headerless samples on stdin: every one
    but a pw-play without --raw (docs/architecture.md, "Playback")."""
    if Path(player).name != "pw-play":
        return True
    try:
        probe = subprocess.run(
            [player, "--help"], capture_output=True, text=True, timeout=5, check=False
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return "--raw" in probe.stdout + probe.stderr


def find_player() -> str | None:
    return next(
        (path for name in PLAYERS if (path := shutil.which(name)) and streams_raw(path)), None
    )


def require_player() -> str:
    player = find_player()
    if player is None:
        raise RuntimeError(
            f"No audio player found ({', '.join(PLAYERS)}; pw-play needs PipeWire newer than 1.0);"
            " pass --output to save a WAV instead."
        )
    return player


def raw_command(player: str, sample_rate: int, audio_device: str | None = None) -> list[str]:
    """Mono float32 samples on stdin, to `audio_device` (a PipeWire or
    PulseAudio sink name, or an ALSA device) or else the default output."""
    name = Path(player).name
    if name not in RAW_COMMANDS:
        raise ValueError(f"Don't know how to stream to {name}.")
    return RAW_COMMANDS[name](player, sample_rate, audio_device)


# Kept small so a stop() wastes little (pinned by
# `test_playback.py::test_synthesis_runs_one_chunk_ahead_of_a_player_that_is_not_reading`).
CHUNKS_SYNTHESIZED_AHEAD = 1


class Speech:
    """Audio playing in the background as it arrives. stop() cuts it off at
    once; wait() blocks until it has finished and raises whatever went wrong.

    A chunk already being synthesized when stop() is called still finishes
    in the background, since the pipeline can't be interrupted, so the next
    call on the same Synthesizer waits for it.

    With `sample_rate` known, the player starts at once instead of after the
    first chunk. `on_first_audio` is called when the first samples have been
    written to it."""

    def __init__(
        self,
        audio: Iterable[Audio],
        player: str | None = None,
        audio_device: str | None = None,
        sample_rate: int | None = None,
        on_first_audio: Callable[[], None] | None = None,
    ) -> None:
        self._player = player or require_player()
        self._audio_device = audio_device
        self._sample_rate = sample_rate
        self._on_first_audio = on_first_audio
        self._stopped = threading.Event()
        self._ready: queue.Queue[Audio | None] = queue.Queue(maxsize=CHUNKS_SYNTHESIZED_AHEAD)
        self._process: subprocess.Popen[bytes] | None = None
        self._process_lock = threading.Lock()
        self._error: BaseException | None = None
        self._synthesis = threading.Thread(
            target=self._synthesize, args=(iter(audio),), daemon=True
        )
        self._playback = threading.Thread(target=self._play, daemon=True)
        self._synthesis.start()
        self._playback.start()

    @property
    def done(self) -> bool:
        return not self._playback.is_alive()

    def stop(self) -> None:
        self._stopped.set()
        with self._process_lock:
            if self._process is not None:
                self._process.kill()

    def wait(self, timeout: float | None = None) -> bool:
        """True once playback has ended, False if `timeout` ran out first."""
        self._playback.join(timeout)
        if self._playback.is_alive():
            return False
        if self._error is not None:
            raise self._error
        return True

    def _hand_over(self, item: Audio | None) -> bool:
        while not self._stopped.is_set():
            try:
                self._ready.put(item, timeout=0.1)
                return True
            except queue.Full:
                continue
        return False

    def _synthesize(self, audio: Iterator[Audio]) -> None:
        try:
            for chunk in audio:
                if not self._hand_over(chunk):
                    return
        except BaseException as error:  # noqa: BLE001 - raised again from wait()
            self._error = error
        self._hand_over(None)

    def _next(self) -> Audio | None:
        while not self._stopped.is_set():
            try:
                return self._ready.get(timeout=0.1)
            except queue.Empty:
                continue
        return None

    def _play(self) -> None:
        try:
            self._write_chunks()
        except BrokenPipeError:
            pass  # stop() killed the player
        except BaseException as error:  # noqa: BLE001 - raised again from wait()
            self._error = self._error or error
        finally:
            self._finish()
            # Frees the synthesis thread from a full queue when no stop() comes
            # (pinned by `test_playback.py::test_a_dead_player_releases_the_synthesis_thread`).
            self._stopped.set()

    def _write_chunks(self) -> None:
        """Each chunk into the player, started at the known sample rate or
        else at the first chunk's, until the audio ends or stop() is called."""
        rate = self._sample_rate
        stdin = self._start(rate) if rate is not None else None
        if rate is not None and stdin is None:
            return
        heard = False
        while (chunk := self._next()) is not None:
            if stdin is None:
                rate = chunk.sample_rate
                stdin = self._start(rate)
                if stdin is None:
                    return
            if chunk.sample_rate != rate:
                raise RuntimeError(
                    f"Speech came at {chunk.sample_rate} Hz to a player started at {rate} Hz."
                )
            stdin.write(chunk.samples.astype("<f4", copy=False).tobytes())
            stdin.flush()
            if not heard and self._on_first_audio is not None:
                self._on_first_audio()
            heard = True

    def _start(self, sample_rate: int) -> IO[bytes] | None:
        """The player's stdin, or None when stop() got there first."""
        with self._process_lock:
            if self._stopped.is_set():
                return None
            self._process = subprocess.Popen(
                raw_command(self._player, sample_rate, self._audio_device), stdin=subprocess.PIPE
            )
            return self._process.stdin

    def _finish(self) -> None:
        if self._process is None:
            return
        if self._process.stdin is not None:
            with contextlib.suppress(BrokenPipeError):
                self._process.stdin.close()
        status = self._process.wait()
        if status != 0 and not self._stopped.is_set() and self._error is None:
            self._error = RuntimeError(f"{Path(self._player).name} exited with status {status}.")
