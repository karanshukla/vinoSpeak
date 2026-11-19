"""Translation before speaking, by SmolLM3-3B on an OpenVINO device: the same
INT4 export vinoWhisper uses, in vinoWhisper's folder so the two share one
copy (1.7 GB), downloaded only when asked for, and loaded only for the first
translation, never at start.

SmolLM3 is trained on English, French, Spanish, Italian, Portuguese and
German; Kokoro speaks the first five of those, so those are the targets."""

from __future__ import annotations

import os
import queue
import re
import shutil
import threading
import time
from collections.abc import Callable, Iterator
from pathlib import Path
from typing import Any, Protocol

from . import model, ovfetch
from .text import chunks

PINS_PATH = Path(__file__).resolve().parent / "translation_pins.json"
LABEL = "SmolLM3-3B"
# Target language: its name in the prompt, and the voice that reads it
# when none in that language was asked for.
TARGETS = {
    "en-us": ("American English", "af_heart"),
    "en-gb": ("British English", "bf_emma"),
    "es": ("Spanish", "ef_dora"),
    "fr-fr": ("French", "ff_siwis"),
    "it": ("Italian", "if_sara"),
    "pt-br": ("Brazilian Portuguese", "pf_dora"),
}
# The NPU's fixed window, as vinoWhisper sizes it for the same model.
NPU_PROPERTIES = {"MAX_PROMPT_LEN": 3072, "MIN_RESPONSE_LEN": 1024}
# Each call translates at most this much, so prompt and answer fit the window
# (pinned by `test_translate.py::test_long_text_is_translated_in_pieces_that_fit_the_window`).
MAX_PIECE_CHARS = 1500
# Tokens for an answer: about one per two characters of a Romance language,
# never past the window's response length.
MIN_NEW_TOKENS = 64
# Unloaded after this long unused, giving back its ~2 GB (pinned by
# `test_translate.py::test_the_model_is_unloaded_once_idle_and_reloaded_when_needed`).
IDLE_UNLOAD_S = 300.0
WATCH_EVERY_S = 30.0
NOT_DOWNLOADED = (
    f"The translation model ({LABEL}, about 1.7 GB) isn't downloaded. "
    "Download it with vinospeak --download-translation-model, or from the window's About page."
)
_THINK = re.compile(r"<think>.*?</think>\s*", re.S)
_OPENING = re.compile(r"\A\s*<text>")


# What vinoWhisper writes beside its copy: the revision once every file
# matched its sha256, and a stamp once it has used it.
VINOWHISPER_REVISION = ".vinowhisper-revision"
VINOWHISPER_VERIFIED = ".vinowhisper-verified"
VINOWHISPER_FLATPAK = "io.github.karanshukla.vinowhisper"
MODEL_NAME = "SmolLM3-3B-int4-cw-ov"


def _data_home() -> Path:
    return Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")


def data_dir() -> Path:
    """vinoSpeak's own translation data (the compiled copy), kept apart from
    the cache: it shouldn't vanish with a cache clean."""
    return _data_home() / "vinospeak" / "translation"


def native_model_dir() -> Path:
    """vinoWhisper's folder for the same pinned model, so installing either
    app after the other leaves one copy. Downloads go here."""
    return _data_home() / "vinowhisper" / "models" / MODEL_NAME


def flatpak_model_dir() -> Path:
    """The same folder inside vinoWhisper's Flatpak, read but never written."""
    flatpak = Path.home() / ".var" / "app" / VINOWHISPER_FLATPAK / "data"
    return flatpak / "vinowhisper" / "models" / MODEL_NAME


def model_dir() -> Path:
    """The first downloaded copy, native before the Flatpak's; the native
    folder when there is none."""
    for directory in (native_model_dir(), flatpak_model_dir()):
        if _holds_the_model(directory):
            return directory
    return native_model_dir()


def compile_cache_dir() -> Path:
    return data_dir() / "ov-cache"


def load_pins() -> model.Pins:
    return model.load_pins(PINS_PATH)


def download_bytes() -> int:
    return sum(int(pin["size"]) for pin in load_pins().files.values())


def _vinowhisper_revision(directory: Path) -> str | None:
    try:
        return (directory / VINOWHISPER_REVISION).read_text(encoding="utf-8").strip()
    except OSError:
        return None


def is_downloaded(directory: Path | None = None) -> bool:
    """Every pinned file there at its pinned size, after a verified download
    by vinoSpeak or by vinoWhisper."""
    return _holds_the_model(directory or model_dir())


def _holds_the_model(directory: Path) -> bool:
    pins = load_pins()
    if directory == flatpak_model_dir():
        return _flatpak_copy_checked(directory, pins)
    checked = model.is_stamped(directory, pins) or _vinowhisper_revision(directory) == pins.revision
    return checked and not model.verify(directory, pins, full=False)


def flatpak_stamp() -> Path:
    return data_dir() / "flatpak-verified"


def _fingerprint(directory: Path, pins: model.Pins) -> str:
    """Size, mtime, ctime and inode of each file: the sandbox can set mtime
    back, not ctime."""
    lines = []
    for name in sorted(pins.files):
        stat = (directory / name).stat()
        lines.append(f"{name} {stat.st_size} {stat.st_mtime_ns} {stat.st_ctime_ns} {stat.st_ino}")
    return "\n".join([pins.revision, *lines]) + "\n"


def _flatpak_copy_checked(directory: Path, pins: model.Pins) -> bool:
    """The Flatpak's folder is writable from its sandbox, so nothing in it is
    trusted: every file is hashed against the pins once, and the result kept
    in vinoSpeak's own folder until a file changes (pinned by
    `test_translate.py::test_a_flatpak_copy_is_hashed_once_and_again_after_a_change`)."""
    if model.verify(directory, pins, full=False):
        return False
    fingerprint = _fingerprint(directory, pins)
    try:
        if flatpak_stamp().read_text(encoding="utf-8") == fingerprint:
            return True
    except OSError:
        pass
    if model.verify(directory, pins, full=True):
        return False
    flatpak_stamp().parent.mkdir(parents=True, exist_ok=True)
    flatpak_stamp().write_text(fingerprint, encoding="utf-8")
    return True


def download(directory: Path | None = None) -> Path:
    """Download and verify the pins, then mark it the way vinoWhisper does,
    so vinoWhisper takes it as its own."""
    directory = directory or native_model_dir()
    pins = load_pins()
    model.download_pinned(directory, pins, "the translation model")
    (directory / VINOWHISPER_REVISION).write_text(pins.revision + "\n", encoding="utf-8")
    return directory


def used_by_vinowhisper(directory: Path | None = None) -> bool:
    directory = directory or model_dir()
    return directory == flatpak_model_dir() or (directory / VINOWHISPER_VERIFIED).is_file()


def disk_usage() -> int:
    return sum(model.disk_usage(d) for d in (model_dir(), data_dir()) if d.is_dir())


def delete() -> tuple[int, bool]:
    """Delete the compiled copy, and the model unless vinoWhisper uses it;
    the bytes freed and whether the model was kept for vinoWhisper."""
    directory = model_dir()
    kept = used_by_vinowhisper(directory)
    freed = model.disk_usage(data_dir()) if data_dir().is_dir() else 0
    shutil.rmtree(data_dir(), ignore_errors=True)
    if not kept and directory.is_dir():
        freed += model.disk_usage(directory)
        shutil.rmtree(directory, ignore_errors=True)
    return freed, kept


def target_name(language: str) -> str:
    if language not in TARGETS:
        raise ValueError(
            f"vinoSpeak can't translate into '{language}'; it translates into {', '.join(TARGETS)}."
        )
    return TARGETS[language][0]


def prompt(language: str) -> str:
    name = target_name(language)
    return (
        f"Translate the text inside <text> into {name}. Reply with the translation "
        "only: no notes, no quotes and no explanation. Keep names, numbers and units as "
        "they are."
    )


def request(text: str, language: str) -> str:
    """The user's turn: the instruction again, the text wrapped
    (docs/architecture.md, Translation; pinned by
    `test_translate.py::test_the_request_repeats_the_language_and_wraps_the_text`)."""
    return f"Translate into {target_name(language)}:\n<text>{text}</text>"


def max_new_tokens(text: str) -> int:
    return min(NPU_PROPERTIES["MIN_RESPONSE_LEN"], max(MIN_NEW_TOKENS, len(text)))


def tidy(answer: str) -> str:
    """The answer without reasoning, the <text> wrapper a small model may echo,
    or anything it adds after the wrapper closes."""
    answer = _OPENING.sub("", _THINK.sub("", answer))
    return answer.partition("</text>")[0].strip()


class Generates(Protocol):
    """vinoWhisper's CleanupModel.generate, which linuxlab's stand-in matches."""

    def generate(self, system: str, user: str, max_new_tokens: int) -> tuple[str, bool]: ...


class SmolLM3:
    """The pinned model in an OpenVINO GenAI LLMPipeline, greedy and without
    SmolLM3's reasoning (`/no_think`)."""

    def __init__(self, device: str, directory: Path | None = None) -> None:
        directory = directory or model_dir()
        if not is_downloaded(directory):
            raise RuntimeError(NOT_DOWNLOADED)
        try:
            import openvino_genai as ov_genai
        except ImportError as error:
            raise RuntimeError("Install vinoSpeak's dependencies with 'uv sync'.") from error
        with_npu = device.startswith("NPU")
        try:
            ovfetch.preload()
        except ovfetch.OvfetchError:
            if with_npu:
                raise
        properties: dict[str, Any] = {"CACHE_DIR": str(compile_cache_dir() / device)}
        if with_npu:
            properties.update(NPU_PROPERTIES)
        self._genai = ov_genai
        self._pipe = ov_genai.LLMPipeline(str(directory), device, **properties)

    def generate(self, system: str, user: str, max_new_tokens: int) -> tuple[str, bool]:
        tokenizer = self._pipe.get_tokenizer()
        history = [
            {"role": "system", "content": system + " /no_think"},
            {"role": "user", "content": user},
        ]
        text = tokenizer.apply_chat_template(history, add_generation_prompt=True)
        settings = self._genai.GenerationConfig()
        settings.max_new_tokens = max_new_tokens
        settings.do_sample = False
        settings.apply_chat_template = False
        answer = str(self._pipe.generate(text, generation_config=settings))
        produced = len(tokenizer.encode(answer).input_ids.data[0])
        return answer, produced >= max_new_tokens


class Translator:
    """Text into one of TARGETS, a piece at a time. Nothing is loaded until
    the first translation, and the model is let go once idle. Calls are
    serialized: the pipeline is not thread-safe."""

    def __init__(
        self,
        device: str = "NPU",
        load: Callable[[], Generates] | None = None,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self.device = device
        self._load = load or (lambda: SmolLM3(device))
        self._clock = clock
        self._model: Generates | None = None
        self._lock = threading.Lock()
        self._last_used = clock()
        self.truncated = False
        self._watching = False

    @property
    def loaded(self) -> bool:
        return self._model is not None

    def translate_piece(self, text: str, language: str) -> str:
        system = prompt(language)
        with self._lock:
            if self._model is None:
                self._model = self._load()
                self._watch()
            try:
                answer, truncated = self._model.generate(
                    system, request(text, language), max_new_tokens(text)
                )
            finally:
                self._last_used = self._clock()
        self.truncated = self.truncated or truncated
        return tidy(answer)

    def pieces(self, text: str) -> list[str]:
        """The first sentence alone, so speech starts soon, then pieces that
        grow up to MAX_PIECE_CHARS."""
        return chunks(text, MAX_PIECE_CHARS)

    def stream(self, text: str, language: str) -> Iterator[str]:
        """Each piece translated, the next one translated while this one is
        spoken; nothing is translated past a consumer that stopped."""
        target_name(language)
        ready: queue.Queue[str | BaseException | None] = queue.Queue(maxsize=1)
        stopped = threading.Event()

        def work() -> None:
            try:
                for piece in self.pieces(text):
                    if stopped.is_set():
                        return
                    _put(ready, self.translate_piece(piece, language), stopped)
            except BaseException as error:  # noqa: BLE001 - raised again by the consumer
                _put(ready, error, stopped)
                return
            _put(ready, None, stopped)

        threading.Thread(target=work, daemon=True, name="translate").start()
        try:
            while (item := ready.get()) is not None:
                if isinstance(item, BaseException):
                    raise item
                if item:
                    yield item
        finally:
            stopped.set()

    def translate(self, text: str, language: str) -> str:
        return "\n\n".join(self.stream(text, language))

    def _watch(self) -> None:
        """Once per process, a thread that lets the model go once idle."""
        if self._watching:
            return
        self._watching = True

        def run() -> None:
            while True:
                time.sleep(WATCH_EVERY_S)
                self.unload_if_idle()

        threading.Thread(target=run, daemon=True, name="translate-idle").start()

    def unload_if_idle(self, idle_s: float = IDLE_UNLOAD_S) -> bool:
        if self._model is None or not self._lock.acquire(blocking=False):
            return False
        try:
            if self._model is None or self._clock() - self._last_used < idle_s:
                return False
            self._model = None
            return True
        finally:
            self._lock.release()


def _put(ready: queue.Queue[Any], item: Any, stopped: threading.Event) -> None:
    while not stopped.is_set():
        try:
            ready.put(item, timeout=0.1)
            return
        except queue.Full:
            continue


def voice_for(language: str, voice: str | None) -> str:
    """The voice asked for, or else the one that reads this language."""
    target_name(language)
    return voice or TARGETS[language][1]
