"""Kokoro text-to-speech using OpenVINO."""

import sys

# Makes OpenVINO's telemetry unimportable, so it must run before anything
# imports openvino (pinned by
# `test_telemetry.py::test_importing_vino_speak_blocks_openvino_telemetry`).
sys.modules.setdefault("openvino_telemetry", None)  # type: ignore[arg-type]

from .playback import Speech  # noqa: E402 - after the telemetry block
from .tts import Audio, Synthesizer, speak, synthesize  # noqa: E402
from .version import installed_version  # noqa: E402

__version__ = installed_version()

__all__ = ["Audio", "Speech", "Synthesizer", "__version__", "speak", "synthesize"]
