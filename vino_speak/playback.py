"""Play speech through the desktop's sound server while it is still being
synthesized."""

from __future__ import annotations

import contextlib
import queue
import shutil
import subprocess
import threading
from collections.abc import Iterable, Iterator
from pathlib import Path
from typing import IO, TYPE_CHECKING

if TYPE_CHECKING:
    from .tts import Audio

# PipeWire first: it is the sound server Wayland desktops run. PulseAudio and
# bare ALSA cover the rest.
PLAYERS = ("pw-play", "paplay", "aplay")


def find_player() -> str | None:
    return next((path for name in PLAYERS if (path := shutil.which(name))), None)


def require_player() -> str:
    player = find_player()
    if player is None:
        raise RuntimeError(
            f"No audio player found ({', '.join(PLAYERS)}); pass --output to save a WAV instead."
        )
    return player


def raw_command(player: str, sample_rate: int, audio_device: str | None = None) -> list[str]:
    """Mono float32 samples on stdin, to `audio_device` (a PipeWire or
    PulseAudio sink name, or an ALSA device) or else the default output."""
    name = Path(player).name
    if name == "pw-play":
        target = [f"--target={audio_device}"] if audio_device else []
        return [
            player,
            "--raw",
            "--format=f32",
            "--channels=1",
            f"--rate={sample_rate}",
            *target,
            "-",
        ]
    if name == "paplay":
        target = [f"--device={audio_device}"] if audio_device else []
        return [
            player,
            "--raw",
            "--format=float32le",
            "--channels=1",
            f"--rate={sample_rate}",
            *target,
        ]
    if name == "aplay":
        target = ["-D", audio_device] if audio_device else []
        return [
            player,
            "-q",
            "-t",
            "raw",
            "-f",
            "FLOAT_LE",
            "-c",
            "1",
            "-r",
            str(sample_rate),
            *target,
        ]
    raise ValueError(f"Don't know how to stream to {name}.")


class Speech:
    """Audio playing in the background as it arrives. stop() cuts it off at
    once; wait() blocks until it has finished and raises whatever went wrong.

    A chunk already being synthesized when stop() is called still finishes
    in the background, since the pipeline can't be interrupted, so the next
    call on the same Synthesizer waits for it."""

    def __init__(
        self,
        audio: Iterable[Audio],
        player: str | None = None,
        audio_device: str | None = None,
    ) -> None:
        self._player = player or require_player()
        self._audio_device = audio_device
        self._stopped = threading.Event()
        # One chunk waiting while the next is synthesized: enough to keep
        # playback fed without spending energy far ahead of a stop()
        self._ready: queue.Queue[Audio | None] = queue.Queue(maxsize=1)
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
        stdin: IO[bytes] | None = None
        try:
            while (chunk := self._next()) is not None:
                if stdin is None:
                    stdin = self._start(chunk.sample_rate)
                    if stdin is None:
                        return
                stdin.write(chunk.samples.astype("<f4", copy=False).tobytes())
                stdin.flush()
        except BrokenPipeError:
            pass  # stop() killed the player
        finally:
            self._finish(stdin)
            # Release the synthesis thread, which otherwise waits on a full
            # queue for a stop() that a library caller may never send.
            self._stopped.set()

    def _start(self, sample_rate: int) -> IO[bytes] | None:
        with self._process_lock:
            if self._stopped.is_set():
                return None
            self._process = subprocess.Popen(
                raw_command(self._player, sample_rate, self._audio_device), stdin=subprocess.PIPE
            )
            return self._process.stdin

    def _finish(self, stdin: IO[bytes] | None) -> None:
        if stdin is not None:
            with contextlib.suppress(BrokenPipeError):
                stdin.close()
        if self._process is None:
            return
        status = self._process.wait()
        if status != 0 and not self._stopped.is_set() and self._error is None:
            self._error = RuntimeError(f"{Path(self._player).name} exited with status {status}.")
