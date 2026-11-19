"""Translation before speaking, by SmolLM3-3B on an OpenVINO device: the same
INT4 export vinoWhisper uses, downloaded by vinoSpeak itself (1.7 GB) only
when asked for, and loaded only for the first translation, never at start.

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
_WRAPPER = re.compile(r"\A\s*<text>\s*|\s*</text>\s*\Z")


def data_dir() -> Path:
    """vinoSpeak's data, kept apart from the cache: a 1.7 GB opt-in shouldn't
    vanish with a cache clean, nor be counted as the Kokoro model."""
    data = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    return data / "vinospeak" / "translation"


def model_dir() -> Path:
    return data_dir() / "SmolLM3-3B-int4-cw-ov"


def compile_cache_dir() -> Path:
    return data_dir() / "ov-cache"


def load_pins() -> model.Pins:
    return model.load_pins(PINS_PATH)


def download_bytes() -> int:
    return sum(int(pin["size"]) for pin in load_pins().files.values())


def is_downloaded(directory: Path | None = None) -> bool:
    """Every pinned file there at its pinned size, after a verified download."""
    directory = directory or model_dir()
    pins = load_pins()
    return model.is_stamped(directory, pins) and not model.verify(directory, pins, full=False)


def download(directory: Path | None = None) -> Path:
    directory = directory or model_dir()
    model.download_pinned(directory, load_pins(), "the translation model")
    return directory


def disk_usage() -> int:
    return model.disk_usage(data_dir()) if data_dir().is_dir() else 0


def delete() -> int:
    """Delete the model and its compiled copy; the bytes freed."""
    freed = disk_usage()
    shutil.rmtree(data_dir(), ignore_errors=True)
    return freed


def target_name(language: str) -> str:
    if language not in TARGETS:
        raise ValueError(
            f"vinoSpeak can't translate into '{language}'; it translates into {', '.join(TARGETS)}."
        )
    return TARGETS[language][0]


def prompt(language: str) -> str:
    name = target_name(language)
    return (
        f"Translate the text the user sends into {name}. Reply with the translation "
        "only: no notes, no quotes and no explanation. Keep names, numbers and units as "
        f"they are. If the text is already in {name}, repeat it unchanged."
    )


def max_new_tokens(text: str) -> int:
    return min(NPU_PROPERTIES["MIN_RESPONSE_LEN"], max(MIN_NEW_TOKENS, len(text)))


def tidy(answer: str) -> str:
    """The answer without reasoning or the <text> wrapper a small model may echo."""
    return _WRAPPER.sub("", _THINK.sub("", answer)).strip()


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
                answer, truncated = self._model.generate(system, text, max_new_tokens(text))
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
