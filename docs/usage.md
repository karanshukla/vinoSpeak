# Usage

Run `vinospeak --help` for the complete list of CLI flags. This guide covers
file handling, translation, SSML, custom lexicons, the HTTP API, model
management, diagnostics, and the Python API.

## How each file is read

| Input | Parsed as |
|---|---|
| `.md`, `.markdown`, `.mdx`, `.mkd` | Markdown |
| `.vwsession` | vinoWhisper session (reads the summary if available, otherwise the transcript) |
| Any other file | Hard-wrapped plain text (a line followed by one starting with a lowercase letter continues the sentence) |
| `-` (stdin) | Plain text |

Use `--kind plain|markdown|document` to override suffix-based detection.
`document` expects text extracted by
[vinoSearch](https://github.com/karanshukla/vinoSearch) from PDFs, DOCX files,
or slides, where each heading, paragraph, or table cell is on its own line and
spoken as a separate sentence. Non-UTF-8 files are rejected.

When reading Markdown:

- Headings, list items, and table rows are spoken as separate sentences.
- Links keep their display text and images are omitted.
- Bare URLs are spoken as their hostname.
- Fenced code blocks are skipped and announced as `"Code skipped."`.

Common abbreviations such as `e.g.` and `Dr.` do not trigger a sentence break.
Pass `--print-text` to inspect the text vinoSpeak will speak and exit without
loading the model.

## Translating

```bash
vinospeak --download-translation-model               # one-time download: SmolLM3-3B, 1.7 GB
vinospeak --file notes.md --translate es              # read in Spanish
vinospeak --file notes.md --translate fr-fr --print-text
vinospeak --file notes.md --translate it --output notes-it.mp3 --subtitles notes-it.srt
vinospeak --delete-translation-model
```

Translation is opt-in and never downloads a model automatically; running
`--translate` without the model prints instructions for fetching it. The model
is [SmolLM3-3B](https://huggingface.co/HuggingFaceTB/SmolLM3-3B) in the same
INT4 export, revision, and hash pins that vinoWhisper uses for cleanup and
summaries.

The model is stored in vinoWhisper's data directory,
`~/.local/share/vinowhisper/models/` (`$XDG_DATA_HOME`), so both applications
share one copy. If vinoWhisper has already downloaded it (natively or as a
Flatpak), vinoSpeak reuses that copy without downloading again. vinoWhisper does
not need to be installed, and `--delete-translation-model` preserves any copy
that vinoWhisper has used.

The translation model loads on demand on the first translation (never at
startup), runs on the same `--device` as Kokoro, and unloads its ~2 GB of memory
after five minutes of inactivity. The first sentence is translated by itself so
playback begins quickly, and the remaining text is translated in chunks of up to
1,500 characters while the preceding chunk plays.

| `--translate` | Default voice (overridden by `--voice`) |
|---|---|
| `es` | `ef_dora` |
| `fr-fr` | `ff_siwis` |
| `it` | `if_sara` |
| `pt-br` | `pf_dora` |
| `en-us`, `en-gb` | `af_heart`, `bf_emma` |

Notes and limitations:

- **Languages:** SmolLM3 does not support Hindi, so `hi` voices only read
  already-Hindi input. German is not offered because Kokoro has no German voice.
- **SSML and lexicon:** Pass plain text rather than SSML when translating. Your
  pronunciation lexicon is applied to the translated text.
- **Accuracy:** A 3B model is good enough for casual listening, but review any
  translation where accuracy matters.
- **Desktop window:** Download the model from the **Translation** row on the
  About page. Once installed, a **Translate** picker appears in the player bar
  and the window displays translated chunks as they arrive, highlighting the
  piece currently being spoken.

## SSML

Input (text, file, or stdin) starting with `<speak` is parsed as SSML. Because
Kokoro does not support pitch, volume, or emphasis controls, only the following
elements are interpreted:

| Element | Behavior |
|---|---|
| `<break time="500ms"/>`, `<break strength="strong"/>` | Pause up to 10 s; strengths from `none` to `x-strong` map to 0–1.25 s |
| `<prosody rate="slow">` | Adjusts speed (`x-slow` 0.5x to `x-fast` 1.5x, `80%`, `+20%`, or `1.2`), multiplied by `--speed` and clamped to 0.5–2.0 |
| `<voice name="bm_george">` | Switches to another voice or blend in its default language |
| `<lang xml:lang="fr-FR">`, `<speak xml:lang="en-GB">` | Overrides the spoken language |
| `<sub alias="World Wide Web Consortium">W3C</sub>` | Speaks the alias text |
| `<say-as interpret-as="characters">API</say-as>` | Spells out the content (`characters`, `spell-out`, `digits`, `telephone`) |
| `<p>`, `<s>` | Marks a sentence boundary |

Other tags (`<emphasis>`, `<mark>`, `<audio>`) have their inner text read as-is.
Input must be well-formed XML (escape `&` as `&amp;`). Use `--print-text` to
preview the parsed text and pauses:

```bash
vinospeak --file script.ssml --print-text
vinospeak --file script.ssml --output script.mp3 --subtitles script.srt
```

## Pronunciations

To fix words Kokoro mispronounces, add respellings to
`~/.config/vinospeak/lexicon.toml` (`$XDG_CONFIG_HOME/vinospeak/`):

```toml
[words]
kubectl = "cube control"
NGINX = "engine x"
"Dr. Who" = "Doctor Who"
```

Matching rules:

- Only whole words are replaced.
- Lowercase entries match case-insensitively; entries with uppercase letters
  match exact case only (so `NGINX` leaves `nginx` untouched).
- When entries overlap, the longest match wins.
- Use `--lexicon FILE` to point to a different TOML file.

OpenVINO GenAI's Kokoro pipeline ignores inline phonemes, so entries must be
plain-text respellings rather than IPA. The GUI worker loads the lexicon at
startup.

## Saving

```bash
vinospeak --file chapter.md --output chapter.opus                       # .wav .flac .ogg .opus .mp3
vinospeak --file chapter.md --output chapter.mp3 --subtitles chapter.vtt
```

The output format is inferred from the filename extension; unsupported suffixes
are rejected before loading the model. WAV and FLAC are 16-bit, and all formats
are mono at Kokoro's 24 kHz sample rate. Text is synthesized chunk by chunk with
a terminal progress line, and the output file is written atomically once
complete.

`--subtitles` writes SubRip (`.srt`) or WebVTT (`.vtt`) cues timed from the
generated audio samples. Each sentence is synthesized individually so cue start
times match playback, which makes saving slightly slower. Sentences longer than
two lines of 42 characters are split across cues with durations proportional to
character length, so mid-sentence cue splits are approximate.

## Voices

```bash
vinospeak --list-voices                                   # id, language, gender, accent
vinospeak "Bonjour." --voice ff_siwis                      # read as French (the voice's language)
vinospeak "Hello." --voice ff_siwis --language en-us       # French voice reading English
vinospeak "Hello." --voice "af_heart(2)+bm_george(1)"       # 2/3 Heart, 1/3 George
```

Each voice ID's leading letter determines its default language, so `--language`
is only needed when reading text in a different language. A blend combines up to
8 voices by weight (weights do not need to sum to 1; `af_heart+af_bella` mixes
them equally) and uses the first voice's language. The GUI worker and Python API
accept the same blend syntax.

## vinoWhisper sessions

```bash
vinospeak --file 2026-10-27-140512.vwsession                     # read the summary
vinospeak --file 2026-10-27-140512.vwsession --part transcript   # read the transcript
```

Machine-generated summaries are prefaced to indicate they are summaries.
Transcripts open with the recording date and strip per-line timestamps.

## OpenAI's speech API

```bash
vinospeak --http                 # serves http://127.0.0.1:8880/v1 (Kokoro-FastAPI's default port)
vinospeak --http 9000 --device CPU
```

The HTTP server keeps one model loaded and implements OpenAI's
`POST /v1/audio/speech` endpoint. Clients configured for OpenAI or
Kokoro-FastAPI only need their base URL changed to `http://127.0.0.1:8880/v1`
(any API key string is accepted on localhost):

```bash
curl http://127.0.0.1:8880/v1/audio/speech -H 'Content-Type: application/json' \
  -d '{"model": "tts-1", "input": "Hello.", "voice": "nova"}' -o hello.mp3
```

| Field | Behavior |
|---|---|
| `input` | Plain text or SSML, respelled using your lexicon; up to 100,000 characters |
| `voice` | Kokoro voice, voice blend, or OpenAI voice name (`alloy`, `nova`, `onyx`, ...) mapped to the matching or closest Kokoro voice |
| `speed` | `0.5` to `2.0` (OpenAI allows `0.25`–`4.0`; values outside Kokoro's range return an error rather than being clamped) |
| `response_format` | `mp3` (default), `opus`, `flac`, `wav`, or `pcm` (24 kHz 16-bit mono, streamed per chunk); `aac` is not supported |
| `model`, `instructions` | Accepted and ignored |

The spoken language is taken from the selected voice. `GET /v1/audio/voices`
lists available voices and `GET /v1/models` exposes the model under OpenAI's
model names. Only `pcm` streams audio as chunks finish; encoded formats are sent
once synthesis completes.

By default the server binds only to `127.0.0.1`. To bind to another address, set
`VINOSPEAK_API_KEY` and send `Authorization: Bearer <key>` on every request; see
[SECURITY.md](../SECURITY.md).

## The model on disk

The Kokoro model is fetched from Hugging Face at a pinned commit and every file
is verified against its SHA-256 digest (see [SECURITY.md](../SECURITY.md)). Both
the model (~160 MB) and OpenVINO's compiled-model cache (so subsequent runs skip
NPU compilation) live under `~/.cache/vinospeak/` (`$XDG_CACHE_HOME/vinospeak/`).

```bash
vinospeak --model-status    # show cached files and disk usage
vinospeak --delete-model    # delete both model and compile cache; next run downloads again
```

The GUI's About page offers the same Delete action. Because vinoPop's read-aloud
feature shares this cached model, deleting it causes vinoPop to re-download it
on next use. A custom `--model-dir` is used as-is without hash checks and is
never deleted by `--delete-model`.

## Check this machine

```bash
vinospeak-doctor          # pass --json for bug reports
```

`vinospeak-doctor` reports installed OpenVINO packages and devices, the selected
audio player, and whether cached model files match their pinned hashes.

If [ovfetch](https://github.com/karanshukla/ovfetch) is on your `PATH`, it also
checks your OpenVINO version against your NPU driver. If your distro's driver
omits the NPU compiler (such as Fedora's `intel-npu-driver` 1.32.0), run
`ovfetch npu install` to place Intel's compiler in your home directory, where
vinoSpeak loads it automatically.

## Python

```python
from vino_speak import Synthesizer

synth = Synthesizer("NPU")  # compiles once; reuse it
audio = synth.synthesize("Hello.")  # audio.samples (float32), audio.sample_rate
speech = synth.speak("A long passage.")  # plays in the background
speech.stop()  # or speech.wait()
```

`Synthesizer` is thread-safe (calls are serialized) and retains no state between
calls apart from loaded voice embeddings. Importing `vino_speak` does not import
OpenVINO or NumPy, keeping startup fast.
