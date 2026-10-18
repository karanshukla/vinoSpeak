"""Kokoro text-to-speech using OpenVINO."""

import sys
from importlib.metadata import PackageNotFoundError, version

# OpenVINO's bundled telemetry (openvino_telemetry, loaded through ovc when
# openvino is imported) takes a missing ~/intel/openvino_telemetry consent
# file as consent and reports usage to Google Analytics. A None entry makes
# it unimportable in this process, so ovc falls back to its no-op stub. Here,
# before the imports below, because every entry point imports the package
# before openvino. The consent file is left alone: it is the user's choice
# for their other OpenVINO tools.
sys.modules.setdefault("openvino_telemetry", None)  # type: ignore[arg-type]

from .playback import Speech  # noqa: E402 - after the telemetry block
from .tts import Audio, Synthesizer, speak, synthesize  # noqa: E402

try:
    __version__ = version("vinospeak")
except PackageNotFoundError:  # running from a source tree that was never installed
    __version__ = "0+unknown"

__all__ = ["Audio", "Speech", "Synthesizer", "__version__", "speak", "synthesize"]
