import re
import sys
import threading
from types import ModuleType

import pytest

from vino_speak import model, translate
from vino_speak.text import chunks


class FakeModel:
    """Translates by tagging the text; `fail_on` raises for that piece."""

    def __init__(self, fail_on=None, truncate=False):
        self.calls = []
        self.fail_on = fail_on
        self.truncate = truncate

    def generate(self, system, request, max_new_tokens):
        user = re.search(r"<text>(.*)</text>", request, re.S).group(1)
        self.calls.append((system, user, max_new_tokens))
        if user == self.fail_on:
            raise RuntimeError("the NPU fell over")
        return f"<think>hmm</think><text>T({user})</text>", self.truncate


@pytest.fixture
def data_home(monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_DATA_HOME", str(tmp_path))
    return tmp_path


def test_the_model_is_vinowhispers_copy_and_the_compiled_one_is_ours(data_home):
    assert translate.model_dir() == data_home / "vinowhisper/models/SmolLM3-3B-int4-cw-ov"
    assert translate.native_model_dir() == translate.model_dir()
    assert translate.compile_cache_dir() == data_home / "vinospeak/translation/ov-cache"


def test_the_pins_are_vinowhispers_smollm3_and_about_1_7_gb():
    pins = translate.load_pins()
    assert pins.model_id == "arankays/SmolLM3-3B-int4-cw-ov"
    assert len(pins.revision) == 40
    assert "openvino_model.bin" in pins.files
    assert 1.6e9 < translate.download_bytes() < 1.8e9


def write_pinned(directory, pins):
    for name, pin in pins.files.items():
        (directory / name).parent.mkdir(parents=True, exist_ok=True)
        (directory / name).write_bytes(b"x" * int(pin["size"]))


@pytest.fixture
def small_pins(monkeypatch):
    pins = model.Pins(
        "arankays/SmolLM3-3B-int4-cw-ov", "a" * 40, {"config.json": {"sha256": "0", "size": 3}}
    )
    monkeypatch.setattr(translate, "load_pins", lambda: pins)
    return pins


def test_downloaded_means_verified_and_stamped_at_the_pinned_sizes(data_home, small_pins):
    directory = translate.model_dir()
    directory.mkdir(parents=True)
    assert not translate.is_downloaded()
    write_pinned(directory, small_pins)
    assert not translate.is_downloaded()
    model._stamp(directory, small_pins)
    assert translate.is_downloaded()
    (directory / "config.json").write_bytes(b"xy")
    assert not translate.is_downloaded()


def test_vinowhispers_checked_copy_counts_as_downloaded(data_home, small_pins):
    directory = translate.model_dir()
    directory.mkdir(parents=True)
    write_pinned(directory, small_pins)
    (directory / ".vinowhisper-revision").write_text("b" * 40 + "\n")
    assert not translate.is_downloaded()
    (directory / ".vinowhisper-revision").write_text(small_pins.revision + "\n")
    assert translate.is_downloaded()
    (directory / "config.json").write_bytes(b"xy")
    assert not translate.is_downloaded()


def flatpak_copy(pins):
    directory = translate.flatpak_model_dir()
    directory.mkdir(parents=True)
    write_pinned(directory, pins)
    (directory / ".vinowhisper-revision").write_text(pins.revision + "\n")
    return directory


def test_a_flatpak_copy_is_hashed_once_and_again_after_a_change(data_home, monkeypatch):
    import hashlib
    import os

    good = b"abc"
    pins = model.Pins(
        "arankays/SmolLM3-3B-int4-cw-ov",
        "a" * 40,
        {"config.json": {"sha256": hashlib.sha256(good).hexdigest(), "size": 3}},
    )
    monkeypatch.setattr(translate, "load_pins", lambda: pins)
    directory = translate.flatpak_model_dir()
    directory.mkdir(parents=True)
    (directory / "config.json").write_bytes(good)
    hashed = []
    real = model.sha256_file
    monkeypatch.setattr(model, "sha256_file", lambda path: hashed.append(path) or real(path))
    assert translate.is_downloaded(directory)
    assert translate.is_downloaded(directory)
    assert len(hashed) == 1, "hashed once, then the fingerprint stands"
    stat = (directory / "config.json").stat()
    (directory / "config.json").write_bytes(b"xyz")
    os.utime(directory / "config.json", ns=(stat.st_atime_ns, stat.st_mtime_ns))
    assert not translate.is_downloaded(directory), "same size and mtime, other bytes"


def test_stamps_inside_the_flatpaks_folder_are_not_trusted(data_home, small_pins):
    directory = flatpak_copy(small_pins)
    model._stamp(directory, small_pins)
    assert not translate.is_downloaded(directory), "small_pins' sha256 can't match"


def test_the_flatpaks_copy_is_found_under_var_app(data_home, small_pins, monkeypatch):
    from pathlib import Path

    assert translate.flatpak_model_dir() == Path.home() / (
        ".var/app/io.github.karanshukla.vinowhisper/data/vinowhisper/models/SmolLM3-3B-int4-cw-ov"
    )
    assert not translate.is_downloaded()
    monkeypatch.setattr(model, "sha256_file", lambda path: "0")
    directory = flatpak_copy(small_pins)
    assert translate.is_downloaded()
    assert translate.model_dir() == directory


def test_a_native_copy_comes_before_the_flatpaks(data_home, small_pins):
    flatpak_copy(small_pins)
    native = translate.native_model_dir()
    native.mkdir(parents=True)
    write_pinned(native, small_pins)
    model._stamp(native, small_pins)
    assert translate.model_dir() == native


def test_delete_never_touches_the_flatpaks_copy(data_home, small_pins, monkeypatch):
    monkeypatch.setattr(model, "sha256_file", lambda path: "0")
    directory = flatpak_copy(small_pins)
    assert translate.is_downloaded()
    stamp = translate.flatpak_stamp().stat().st_size
    assert translate.delete() == (stamp, True)
    assert (directory / "config.json").is_file()


def test_download_fetches_the_pins_and_marks_them_as_vinowhisper_would(
    data_home, small_pins, monkeypatch
):
    fetched = []

    def fetch(directory, pins, what):
        directory.mkdir(parents=True)
        fetched.append((directory, what))

    monkeypatch.setattr(model, "download_pinned", fetch)
    assert translate.download() == translate.model_dir()
    assert fetched == [(translate.model_dir(), "the translation model")]
    marker = translate.model_dir() / ".vinowhisper-revision"
    assert marker.read_text().strip() == small_pins.revision


def fill(data_home):
    translate.model_dir().mkdir(parents=True)
    (translate.model_dir() / "config.json").write_bytes(b"abc")
    translate.compile_cache_dir().mkdir(parents=True)
    (translate.compile_cache_dir() / "blob").write_bytes(b"12345")


def test_delete_removes_the_model_and_its_compiled_copy(data_home, small_pins):
    fill(data_home)
    assert translate.disk_usage() == 8
    assert translate.delete() == (8, False)
    assert not translate.data_dir().exists()
    assert not translate.model_dir().exists()
    assert translate.delete() == (0, False)


def test_delete_keeps_a_model_vinowhisper_uses(data_home, small_pins):
    fill(data_home)
    (translate.model_dir() / ".vinowhisper-verified").write_text("")
    assert translate.delete() == (5, True)
    assert (translate.model_dir() / "config.json").is_file()
    assert not translate.data_dir().exists()


def test_smollm3_kokoro_languages_are_the_targets_and_hindi_is_not():
    assert set(translate.TARGETS) == {"en-us", "en-gb", "es", "fr-fr", "it", "pt-br"}
    with pytest.raises(ValueError, match="can't translate into 'hi'"):
        translate.prompt("hi")


def test_each_target_has_a_voice_of_its_own_language():
    from vino_speak import voices

    for language, (_, voice) in translate.TARGETS.items():
        assert voices.language_of(voice) == language


def test_a_voice_asked_for_is_kept_otherwise_the_languages_reads():
    assert translate.voice_for("es", None) == "ef_dora"
    assert translate.voice_for("es", "bm_george") == "bm_george"


def test_the_prompt_names_the_language_and_asks_for_the_translation_only():
    assert "into Spanish" in translate.prompt("es")
    assert "translation only" in translate.prompt("es")


def test_the_request_repeats_the_language_and_wraps_the_text():
    assert translate.request("Good morning.", "es") == (
        "Translate into Spanish:\n<text>Good morning.</text>"
    )
    with pytest.raises(ValueError):
        translate.request("Hi.", "hi")


def test_the_answer_loses_its_reasoning_and_wrapper():
    assert translate.tidy("<think>\nwhy\n</think>\n<text> Hola. </text>\n") == "Hola."
    assert translate.tidy("Hola.") == "Hola."


def test_brackets_around_the_whole_answer_are_dropped():
    assert (
        translate.tidy("<Tech Outage Afecta las Operaciones.>")
        == "Tech Outage Afecta las Operaciones."
    )
    assert translate.tidy("<Ciao>, mi chiamo Maria.") == "<Ciao>, mi chiamo Maria."


def test_a_note_after_the_wrapper_closes_is_not_read_aloud():
    answer = "How are you today?</text>\n\nThe translation into British English is the same."
    assert translate.tidy(answer) == "How are you today?"


def test_new_tokens_grow_with_the_text_within_the_window():
    assert translate.max_new_tokens("Hi.") == translate.MIN_NEW_TOKENS
    assert translate.max_new_tokens("x" * 500) == 500
    assert translate.max_new_tokens("x" * 5000) == translate.NPU_PROPERTIES["MIN_RESPONSE_LEN"]


def test_nothing_loads_until_the_first_translation():
    loads = []
    translator = translate.Translator("CPU", load=lambda: loads.append(1) or FakeModel())
    assert not translator.loaded and loads == []
    assert translator.translate("Hello.", "es") == "T(Hello.)"
    assert translator.translate("Again.", "es") == "T(Again.)"
    assert loads == [1]


def test_long_text_is_translated_in_pieces_that_fit_the_window():
    fake = FakeModel()
    translator = translate.Translator("CPU", load=lambda: fake)
    sentence = "This sentence is here to be about one hundred characters long so the piece sizes are easy. "
    text = sentence * 40
    translator.translate(text, "fr-fr")
    pieces = [user for _, user, _ in fake.calls]
    assert pieces == chunks(text, translate.MAX_PIECE_CHARS)
    assert len(pieces[0]) < 100
    assert max(map(len, pieces)) <= translate.MAX_PIECE_CHARS
    assert all("into French" in system for system, _, _ in fake.calls)


def test_stream_yields_each_piece_and_raises_what_went_wrong():
    translator = translate.Translator("CPU", load=lambda: FakeModel(fail_on="Two is here."))
    stream = translator.stream("One. Two is here.", "es")
    assert next(stream) == "T(One.)"
    with pytest.raises(RuntimeError, match="fell over"):
        next(stream)


def test_a_consumer_that_stops_stops_the_translation():
    fake = FakeModel()
    gate = threading.Event()
    original = fake.generate

    def slow(system, user, tokens):
        gate.wait(5)
        return original(system, user, tokens)

    fake.generate = slow
    sentence = "This sentence is here to be about one hundred characters long so the piece sizes are easy. "
    stream = translate.Translator("CPU", load=lambda: fake).stream(sentence * 60, "es")
    gate.set()
    next(stream)
    stream.close()
    import time

    time.sleep(0.5)
    assert len(fake.calls) <= 3


def test_an_unknown_target_is_refused_before_anything_loads():
    translator = translate.Translator("CPU", load=lambda: pytest.fail("loaded"))
    with pytest.raises(ValueError, match="can't translate"):
        list(translator.stream("Hi.", "ja"))


def test_a_cut_off_answer_is_remembered():
    translator = translate.Translator("CPU", load=lambda: FakeModel(truncate=True))
    translator.translate("Hi.", "es")
    assert translator.truncated


def test_the_model_is_unloaded_once_idle_and_reloaded_when_needed():
    now = [0.0]
    loads = []
    translator = translate.Translator(
        "CPU", load=lambda: loads.append(1) or FakeModel(), clock=lambda: now[0]
    )
    assert not translator.unload_if_idle()
    translator.translate("Hi.", "es")
    now[0] = translate.IDLE_UNLOAD_S - 1
    assert not translator.unload_if_idle()
    now[0] = translate.IDLE_UNLOAD_S
    assert translator.unload_if_idle()
    assert not translator.loaded
    translator.translate("Hi.", "es")
    assert loads == [1, 1]


class FakeTokens:
    def __init__(self, n):
        self.input_ids = type("Ids", (), {"data": [list(range(n))]})()


class FakeTokenizer:
    def apply_chat_template(self, history, add_generation_prompt):
        assert add_generation_prompt
        return "|".join(f"{m['role']}:{m['content']}" for m in history)

    def encode(self, text):
        return FakeTokens(len(text.split()))


class FakeLLMPipeline:
    made = []

    def __init__(self, directory, device, **properties):
        self.directory, self.device, self.properties = directory, device, properties
        self.prompts = []
        FakeLLMPipeline.made.append(self)

    def get_tokenizer(self):
        return FakeTokenizer()

    def generate(self, text, generation_config):
        self.prompts.append((text, generation_config))
        return "uno dos tres"


@pytest.fixture
def fake_genai(monkeypatch, data_home):
    genai = ModuleType("openvino_genai")
    genai.LLMPipeline = FakeLLMPipeline
    genai.GenerationConfig = type("GenerationConfig", (), {})
    monkeypatch.setitem(sys.modules, "openvino_genai", genai)
    monkeypatch.setattr(translate, "is_downloaded", lambda directory=None: True)
    FakeLLMPipeline.made.clear()


@pytest.mark.parametrize(("device", "npu"), [("NPU", True), ("CPU", False)])
def test_smollm3_gets_the_npu_window_only_on_the_npu(fake_genai, device, npu):
    translate.SmolLM3(device)
    [pipe] = FakeLLMPipeline.made
    assert pipe.device == device
    assert pipe.properties["CACHE_DIR"] == str(translate.compile_cache_dir() / device)
    assert ("MAX_PROMPT_LEN" in pipe.properties) is npu


def test_smollm3_asks_greedily_without_reasoning_and_says_when_it_was_cut(fake_genai):
    smol = translate.SmolLM3("CPU")
    answer, truncated = smol.generate("Translate.", "one two three", 3)
    text, settings = FakeLLMPipeline.made[0].prompts[0]
    assert text == "system:Translate. /no_think|user:one two three"
    assert settings.do_sample is False and settings.apply_chat_template is False
    assert settings.max_new_tokens == 3
    assert (answer, truncated) == ("uno dos tres", True)
    assert smol.generate("Translate.", "one", 4)[1] is False


def test_smollm3_refuses_a_model_that_isnt_downloaded(data_home):
    with pytest.raises(RuntimeError, match="isn't downloaded"):
        translate.SmolLM3("CPU")


def test_the_window_knows_the_same_targets_voices_and_size():
    import json
    from pathlib import Path

    table = json.loads((Path(__file__).parent / "fixtures/shared_with_window.json").read_text())
    shared = table["translation"]
    assert shared["model_dir"] == translate.model_dir().name
    assert shared["revision"] == translate.load_pins().revision
    assert shared["download_bytes"] == translate.download_bytes()
    assert shared["targets"] == {
        language: voice for language, (_, voice) in translate.TARGETS.items()
    }
