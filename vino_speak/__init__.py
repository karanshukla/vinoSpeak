"""Kokoro text-to-speech using OpenVINO."""

from importlib.metadata import PackageNotFoundError, version

from .playback import Speech
from .tts import Audio, Synthesizer, speak, synthesize

try:
    __version__ = version("vinospeak")
except PackageNotFoundError:  # running from a source tree that was never installed
    __version__ = "0+unknown"

__all__ = ["Audio", "Speech", "Synthesizer", "__version__", "speak", "synthesize"]
