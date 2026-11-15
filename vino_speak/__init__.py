"""Kokoro text-to-speech using OpenVINO."""

import sys
from importlib.metadata import PackageNotFoundError, version

# Makes OpenVINO's telemetry unimportable; must run before anything imports
# openvino (docs/architecture.md, "Telemetry"; pinned by tests/test_telemetry.py).
sys.modules.setdefault("openvino_telemetry", None)  # type: ignore[arg-type]

from .playback import Speech  # noqa: E402 - after the telemetry block
from .tts import Audio, Synthesizer, speak, synthesize  # noqa: E402

try:
    __version__ = version("vinospeak")
except PackageNotFoundError:  # running from a source tree that was never installed
    __version__ = "0+unknown"

__all__ = ["Audio", "Speech", "Synthesizer", "__version__", "speak", "synthesize"]
