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

## SSML

Text (or a file, or stdin) that starts with `<speak` is read as SSML. Kokoro
can't do pitch, volume or emphasis, so only what it can honour is understood:

| Element | What it does |
|---|---|
| `<break time="500ms"/>`, `<break strength="strong"/>` | A pause, up to 10 s; strengths `none` to `x-strong` are 0 to 1.25 s |
| `<prosody rate="slow">` | Speed: `x-slow` 0.5x to `x-fast` 1.5x, `80%`, `+20%` or `1.2`, times `--speed`, held within 0.5 to 2 |
| `<voice name="bm_george">` | Another voice or blend, in its own language |
| `<lang xml:lang="fr-FR">`, `<speak xml:lang="en-GB">` | Read as that language |
| `<sub alias="World Wide Web Consortium">W3C</sub>` | Say the alias |
| `<say-as interpret-as="characters">API</say-as>` | Spell it out (`characters`, `spell-out`, `digits`, `telephone`) |
| `<p>`, `<s>` | End a sentence |

Any other element (`<emphasis>`, `<mark>`, `<audio>`) is read as its text. It
has to be well-formed XML, so `&` is `&amp;`. `--print-text` shows the text and
the pauses:

```bash
vinospeak --file script.ssml --print-text
vinospeak --file script.ssml --output script.mp3 --subtitles script.srt
```

## Pronunciations

Put the words Kokoro says wrong in `~/.config/vinospeak/lexicon.toml`
(`$XDG_CONFIG_HOME/vinospeak/`), respelled as it should say them:

```toml
[words]
kubectl = "cube control"
NGINX = "engine x"
"Dr. Who" = "Doctor Who"
```

Only whole words are respelled. An entry in lowercase matches any case; one
with a capital matches only as written, so `NGINX` leaves `nginx` alone. When
two entries overlap the longer one wins. `--lexicon FILE` uses another file
instead. OpenVINO GenAI's Kokoro ignores inline phonemes, so an entry is a
respelling, not IPA. The window's worker reads the lexicon when it starts.

## Saving

```bash
vinospeak --file chapter.md --output chapter.opus                       # .wav .flac .ogg .opus .mp3
vinospeak --file chapter.md --output chapter.mp3 --subtitles chapter.vtt
```

The suffix picks the format; anything else is refused before the model
loads. WAV and FLAC are 16-bit, all of them mono at Kokoro's 24 kHz. The text
is synthesized a chunk at a time, with a progress line in a terminal, and the
file only appears under its name once it is whole.

`--subtitles` writes SubRip (`.srt`) or WebVTT (`.vtt`) timed from the
samples. Each sentence is synthesized on its own so its cue starts when it
does, which makes the save slightly slower. A sentence longer than two lines
of 42 characters is split into cues that share its time by length, so those
cues are close, not exact.

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

## OpenAI's speech API

```bash
vinospeak --http                 # http://127.0.0.1:8880/v1, Kokoro-FastAPI's port
vinospeak --http 9000 --device CPU
```

It keeps one model loaded and answers what OpenAI's `POST /v1/audio/speech`
takes, so a client set up for OpenAI or Kokoro-FastAPI only needs its base URL
changed to `http://127.0.0.1:8880/v1` (any API key will do):

```bash
curl http://127.0.0.1:8880/v1/audio/speech -H 'Content-Type: application/json' \
  -d '{"model": "tts-1", "input": "Hello.", "voice": "nova"}' -o hello.mp3
```

| Field | Taken as |
|---|---|
| `input` | The text, plain or SSML, respelled by your lexicon; up to 100,000 characters |
| `voice` | A Kokoro voice or blend, or OpenAI's name (`alloy`, `nova`, `onyx`, ...) for the Kokoro voice of that name or the nearest kind |
| `speed` | 0.5 to 2 (OpenAI allows 0.25 to 4; outside Kokoro's range is refused, not changed) |
| `response_format` | `mp3` (default), `opus`, `flac`, `wav`, or `pcm`: 24 kHz 16-bit mono, streamed as each chunk is ready. No `aac` |
| `model`, `instructions` | Accepted and ignored |

The language is the voice's own. `GET /v1/audio/voices` lists the voices and
`GET /v1/models` the model under OpenAI's names. Only `pcm` starts sending
before the whole text is synthesized; the encoded formats are sent once the
text is done.

It listens on 127.0.0.1 only. To serve another address, set
`VINOSPEAK_API_KEY` and send it as `Authorization: Bearer ...`; see
[SECURITY.md](../SECURITY.md).

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
