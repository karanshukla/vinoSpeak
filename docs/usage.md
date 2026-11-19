# Usage

`vinospeak --help` lists every option. This page covers what the help text
doesn't.

## How each file is read

| File | Read as |
|---|---|
| `.md`, `.markdown`, `.mdx`, `.mkd` | Markdown |
| `.vwsession` | A vinoWhisper session: its summary if it has one, else its transcript |
| Any other file | Text hard-wrapped at a width: a line followed by one starting in lowercase carries on the sentence |
| `-` (stdin) | Plain text |

`--kind plain|markdown|document` overrides the suffix. `document` is the text
[vinoSearch](https://github.com/karanshukla/vinoSearch) extracts from a PDF,
DOCX or slides: one heading, paragraph or table cell a line, each its own
sentence. A file that isn't UTF-8 text is refused.

In markdown:

- Headings, list items and table rows become sentences.
- Links keep their text, and images go.
- URLs are said as their host.
- Code blocks are skipped ("Code skipped.").

Abbreviations like `e.g.` and `Dr.` don't end a sentence.

`--print-text` prints what would be spoken and exits, without loading the
model.

## Voices

```bash
vinospeak --list-voices                                   # id, language, gender, accent
vinospeak "Bonjour." --voice ff_siwis                      # read as French, the voice's language
vinospeak "Hello." --voice ff_siwis --language en-us       # a French voice reading English
vinospeak "Hello." --voice "af_heart(2)+bm_george(1)"       # two thirds Heart, one third George
```

A voice's first letter says its language, so `--language` is only needed to
read text in another one. A blend mixes up to 8 voices by weight (the
weights needn't add up to anything; `af_heart+af_bella` is half each) and
reads in its first voice's language. The window's worker and the Python API
take the same blends.

## vinoWhisper sessions

```bash
vinospeak --file 2026-10-27-140512.vwsession                     # the summary
vinospeak --file 2026-10-27-140512.vwsession --part transcript
```

A machine-written summary is said to be one before it is read. The transcript
starts with the day it was recorded, and its timestamps are dropped.

## The model on disk

The model is fetched from Hugging Face at one pinned commit, and every file is
checked against its sha256 (see [SECURITY.md](../SECURITY.md)). It lives in
`~/.cache/vinospeak/` (`$XDG_CACHE_HOME/vinospeak/`), along with OpenVINO's
compiled-model cache so later runs skip the NPU compile: about 160 MB plus
whatever the compile leaves for each device.

```bash
vinospeak --model-status    # what's there, and its size
vinospeak --delete-model    # delete both; the next run downloads the model again
```

The window's About page has the same Delete button. vinoPop's read-aloud uses
this copy too, so it downloads the model again after a delete. A `--model-dir`
of your own is used unchecked and never deleted.

## Check this machine

```bash
vinospeak-doctor          # --json for a bug report
```

It shows the OpenVINO packages and devices, the audio player it picked, and
whether the cached model still matches its pins.

With [ovfetch](https://github.com/karanshukla/ovfetch) on your `PATH` it also
checks your OpenVINO version against your NPU driver. If your distro's driver
has no NPU compiler (Fedora's `intel-npu-driver` 1.32.0), `ovfetch npu install`
fetches Intel's into your home directory and vinoSpeak loads it from there.

## Python

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles once; reuse it
audio = synth.synthesize("Hello.")  # audio.samples (float32), audio.sample_rate
speech = synth.speak("A long passage.")  # plays in the background
speech.stop()  # or speech.wait()
```

`Synthesizer` is thread-safe (calls are serialized) and keeps no state between
calls beyond the loaded voices. Importing `vino_speak` imports neither OpenVINO
nor numpy, so it is cheap to import at start-up.
