"""Play a WAV through the desktop's sound server."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

# PipeWire first: it is the sound server Wayland desktops run. PulseAudio and
# bare ALSA cover the rest.
PLAYERS = ("pw-play", "paplay", "aplay")


def find_player() -> str | None:
    return next((path for name in PLAYERS if (path := shutil.which(name))), None)


def play(wav: Path) -> None:
    player = find_player()
    if player is None:
        raise RuntimeError(
            f"No audio player found ({', '.join(PLAYERS)}); pass --output to save a WAV instead."
        )
    try:
        subprocess.run([player, str(wav)], check=True)
    except subprocess.CalledProcessError as error:
        raise RuntimeError(f"{Path(player).name} exited with status {error.returncode}.") from error
