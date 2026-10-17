"""Kokoro text-to-speech using OpenVINO."""

from importlib.metadata import PackageNotFoundError, version

from .tts import Audio, Synthesizer, synthesize

try:
    __version__ = version("vinospeak")
except PackageNotFoundError:  # running from a source tree that was never installed
    __version__ = "0+unknown"

__all__ = ["Audio", "Synthesizer", "__version__", "synthesize"]
