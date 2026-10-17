# vinoSpeak

Intel NPU-powered text-to-speech using the Kokoro-82M INT8 OpenVINO model.

## Requirements

- Python 3.11 or newer
- An Intel NPU with a compatible driver and OpenVINO 2026.4 or newer
- Internet access on first run to download the model

## Install

Create and activate a virtual environment, then install vinoSpeak:

```bash
python -m venv .venv
source .venv/bin/activate
python -m pip install -e .
```

## Synthesize speech

The first run downloads `OpenVINO/Kokoro-82M-int8-ov` from Hugging Face. The
model is cached under `~/.cache/vinospeak/` by default.

```bash
vino-speak "Hello from vinoSpeak." --output hello.wav
```

The default inference device is the Intel NPU. Choose a different OpenVINO
device, voice, language, speed, or model directory with the corresponding
options:

```bash
vino-speak "A sample sentence." \
  --device NPU \
  --voice af_heart \
  --language en-us \
  --speed 1.0 \
  --model-dir ./kokoro-model \
  --output speech.wav
```

Use `vino-speak --help` to see all options. The generated WAV uses the sample
rate reported by OpenVINO GenAI.
