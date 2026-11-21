import sys
from types import ModuleType

import numpy as np
import pytest
from conftest import FakeOpenVINO, FakeTensor

from vino_speak import model, tts


class FakeRemoteSpeech:
    element_type = "f32"
    shape = (2,)

    @property
    def data(self):
        raise RuntimeError("Remote tensor is not host-accessible")

    def copy_to(self, host_tensor):
        host_tensor.data[:] = (0.25, -0.25)


@pytest.mark.parametrize(
    ("text", "voice", "speed", "language"),
    [
        ("  ", "af_heart", 1.0, "en-us"),
        ("Hello", "../voice", 1.0, "en-us"),
        ("Hello", "af_heart", 2.1, "en-us"),
        ("Hello", "af_heart", 0.4, "en-us"),
        ("Hello", "af_heart", 1.0, "klingon"),
    ],
)
def test_validate_inputs_rejects_bad_requests(text, voice, speed, language):
    with pytest.raises(ValueError):
        tts.validate_inputs(text, voice, speed, language)


def test_validate_inputs_accepts_the_defaults():
    tts.validate_inputs("Hello", tts.DEFAULT_VOICE, 1.0)


def test_load_voice_embedding_checks_size_and_shape(tmp_path):
    voice_path = tmp_path / "voice.bin"
    np.arange(4, dtype=np.float32).tofile(voice_path)

    embedding = tts.load_voice_embedding(voice_path, (2, 2), FakeOpenVINO)

    np.testing.assert_array_equal(embedding.data, [[0, 1], [2, 3]])
    with pytest.raises(ValueError, match="expects 3"):
        tts.load_voice_embedding(voice_path, (3,), FakeOpenVINO)


def test_audio_data_copies_remote_tensor_to_host():
    audio = tts.get_audio_data(FakeRemoteSpeech(), FakeOpenVINO)
    np.testing.assert_array_equal(audio, [0.25, -0.25])


@pytest.mark.parametrize("values", [[], [float("nan")]])
def test_audio_data_rejects_empty_or_non_finite_waveforms(values):
    with pytest.raises(RuntimeError, match="empty or contains non-finite"):
        tts.get_audio_data(FakeTensor(np.asarray(values)), FakeOpenVINO)


class FakePipeline:
    instances: list["FakePipeline"] = []

    def __init__(self, model_dir, device, **properties):
        self.model_dir = model_dir
        self.device = device
        self.cache_dir = properties.pop("CACHE_DIR", None)
        self.properties = properties
        self.calls = []
        FakePipeline.instances.append(self)

    def get_speaker_embedding_shape(self):
        return (2,)

    def generate(self, text, embedding, language, speed):
        self.calls.append((text, language, speed))
        speech = FakeTensor(np.asarray([0.1, -0.1], dtype=np.float32))
        return type("Result", (), {"speeches": [speech], "output_sample_rate": 24000})()


@pytest.fixture
def fake_runtime(monkeypatch, tmp_path):
    openvino = ModuleType("openvino")
    openvino.Tensor = FakeTensor
    genai = ModuleType("openvino_genai")
    genai.Text2SpeechPipeline = FakePipeline
    loader = ModuleType("espeakng_loader")
    loader.get_library_path = lambda: "/espeak/lib"
    loader.get_data_path = lambda: "/espeak/data"
    for name, module in (
        ("openvino", openvino),
        ("openvino_genai", genai),
        ("espeakng_loader", loader),
    ):
        monkeypatch.setitem(sys.modules, name, module)
    monkeypatch.setenv("MISAKI_ESPEAK_LIBRARY", "/already/set")
    FakePipeline.instances.clear()

    model_dir = tmp_path / "model"
    for filename in model.MODEL_FILES:
        (model_dir / filename).parent.mkdir(parents=True, exist_ok=True)
        (model_dir / filename).touch()
    np.arange(2, dtype=np.float32).tofile(model_dir / "voices" / "af_heart.bin")
    return model_dir


def test_synthesizer_returns_audio_and_reuses_the_pipeline(fake_runtime):
    synthesizer = tts.Synthesizer("CPU", fake_runtime)

    first = synthesizer.synthesize("Hello")
    synthesizer.synthesize("Again", speed=1.5)

    assert first.sample_rate == 24000
    np.testing.assert_allclose(first.samples, [0.1, -0.1])
    assert len(FakePipeline.instances) == 1
    pipeline = FakePipeline.instances[0]
    assert pipeline.device == "CPU"
    assert pipeline.calls == [("Hello", "en-us", 1.0), ("Again", "en-us", 1.5)]


@pytest.mark.parametrize(
    ("device", "properties"),
    [("NPU", {"NPUW_SUBMODEL_DEVICE": ""}), ("CPU", {}), ("GPU", {})],
)
def test_synthesizer_unpins_the_vocoder_only_on_the_npu(fake_runtime, device, properties):
    tts.Synthesizer(device, fake_runtime)
    assert FakePipeline.instances[0].properties == properties


def test_every_pipeline_shares_the_compile_cache(fake_runtime, monkeypatch, tmp_path):
    monkeypatch.setenv("XDG_CACHE_HOME", str(tmp_path / "cache"))
    tts.Synthesizer("NPU", fake_runtime)
    assert FakePipeline.instances[0].cache_dir == str(tmp_path / "cache/vinospeak/ov-cache")


def test_synthesizer_does_not_override_an_espeak_path_already_set(fake_runtime, monkeypatch):
    monkeypatch.delenv("ESPEAK_DATA_PATH", raising=False)
    tts.Synthesizer("CPU", fake_runtime)
    assert tts.os.environ["MISAKI_ESPEAK_LIBRARY"] == "/already/set"
    assert tts.os.environ["ESPEAK_DATA_PATH"] == "/espeak/data"


def test_synthesizer_rejects_an_unknown_voice(fake_runtime):
    with pytest.raises(ValueError, match="was not found"):
        tts.Synthesizer("CPU", fake_runtime).synthesize("Hello", voice="af_nobody")


def test_synthesize_validates_before_loading_anything(tmp_path):
    with pytest.raises(ValueError, match="empty"):
        tts.synthesize(" ", tmp_path / "out.wav", model_dir=tmp_path / "never-created")
    assert not (tmp_path / "never-created").exists()


def test_stream_synthesizes_a_chunk_at_a_time_as_it_is_consumed(fake_runtime):
    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    stream = synthesizer.stream("First one. Second one.\n\nThird.")
    pipeline = FakePipeline.instances[0]
    assert pipeline.calls == []
    assert next(stream).sample_rate == 24000
    assert [call[0] for call in pipeline.calls] == ["First one."]
    list(stream)
    assert [call[0] for call in pipeline.calls] == ["First one.", "Second one. Third."]


def test_stream_validates_before_anything_is_consumed(fake_runtime):
    with pytest.raises(ValueError, match="Speed"):
        tts.Synthesizer("CPU", fake_runtime).stream("Hello", speed=9)


def test_speak_checks_for_a_player_before_loading_the_model(monkeypatch, tmp_path):
    def no_player():
        raise RuntimeError("no player")

    monkeypatch.setattr(tts, "require_player", no_player)
    with pytest.raises(RuntimeError, match="no player"):
        tts.speak("Hello", model_dir=tmp_path / "never-created")
    assert not (tmp_path / "never-created").exists()


def test_speak_validates_before_looking_for_a_player(monkeypatch, tmp_path):
    monkeypatch.setattr(tts, "require_player", lambda: pytest.fail("looked for a player"))
    with pytest.raises(ValueError, match="empty"):
        tts.speak(" ", model_dir=tmp_path / "never-created")
    assert not (tmp_path / "never-created").exists()


@pytest.mark.parametrize("speed", [tts.MIN_SPEED, tts.MAX_SPEED])
def test_the_slowest_and_fastest_speeds_are_accepted(speed):
    tts.validate_inputs("Hello", "af_heart", speed)


@pytest.mark.parametrize("speed", [0.49, 2.01])
def test_a_speed_just_outside_the_range_is_refused(speed):
    with pytest.raises(ValueError, match="between 0.5 and 2.0"):
        tts.validate_inputs("Hello", "af_heart", speed)


def fail_with_override(monkeypatch, method):
    """Make FakePipeline's `method` raise while the vocoder pin is overridden."""
    real = getattr(FakePipeline, method)

    def failing(self, *args, **kwargs):
        result = real(self, *args, **kwargs)
        if "NPUW_SUBMODEL_DEVICE" in self.properties:
            raise RuntimeError("Failed to compile KokoroModelB_8")
        return result

    monkeypatch.setattr(FakePipeline, method, failing)


def test_an_npu_that_cant_compile_the_vocoder_falls_back_to_the_pin(fake_runtime, monkeypatch):
    fail_with_override(monkeypatch, "__init__")
    with pytest.warns(RuntimeWarning, match="KokoroModelB_8"):
        synthesizer = tts.Synthesizer("NPU", fake_runtime)
    assert synthesizer.synthesize("Hello").sample_rate == 24000
    assert [p.properties for p in FakePipeline.instances] == [{"NPUW_SUBMODEL_DEVICE": ""}, {}]


def test_a_first_generate_that_fails_on_the_npu_retries_with_the_pin(fake_runtime, monkeypatch):
    fail_with_override(monkeypatch, "generate")
    synthesizer = tts.Synthesizer("NPU", fake_runtime)
    with pytest.warns(RuntimeWarning, match="KokoroModelB_8"):
        audio = synthesizer.synthesize("Hello")
    np.testing.assert_allclose(audio.samples, [0.1, -0.1])
    assert [p.properties for p in FakePipeline.instances] == [{"NPUW_SUBMODEL_DEVICE": ""}, {}]
    assert FakePipeline.instances[1].calls == [("Hello", "en-us", 1.0)]


def test_non_finite_audio_on_the_npu_retries_with_the_pin(fake_runtime, monkeypatch):
    real = FakePipeline.generate

    def nan_with_override(self, *args, **kwargs):
        result = real(self, *args, **kwargs)
        if "NPUW_SUBMODEL_DEVICE" in self.properties:
            result.speeches = [FakeTensor(np.asarray([float("nan")], dtype=np.float32))]
        return result

    monkeypatch.setattr(FakePipeline, "generate", nan_with_override)
    synthesizer = tts.Synthesizer("NPU", fake_runtime)
    with pytest.warns(RuntimeWarning, match="non-finite"):
        np.testing.assert_allclose(synthesizer.synthesize("Hello").samples, [0.1, -0.1])


def test_once_the_override_has_worked_errors_are_not_retried(fake_runtime, monkeypatch):
    synthesizer = tts.Synthesizer("NPU", fake_runtime)
    synthesizer.synthesize("Hello")

    def broken(self, *args, **kwargs):
        raise RuntimeError("NPU went away")

    monkeypatch.setattr(FakePipeline, "generate", broken)
    with pytest.raises(RuntimeError, match="NPU went away"):
        synthesizer.synthesize("Again")
    assert len(FakePipeline.instances) == 1


def test_other_devices_never_retry(fake_runtime, monkeypatch):
    def broken(self, *args, **kwargs):
        raise RuntimeError("boom")

    monkeypatch.setattr(FakePipeline, "generate", broken)
    with pytest.raises(RuntimeError, match="boom"):
        tts.Synthesizer("CPU", fake_runtime).synthesize("Hello")
    assert len(FakePipeline.instances) == 1


@pytest.mark.parametrize(("device", "fatal"), [("NPU", True), ("NPU.0", True), ("CPU", False)])
def test_a_prefix_that_will_not_load_is_fatal_only_on_the_npu(
    fake_runtime, monkeypatch, device, fatal
):
    def broken_preload():
        raise tts.ovfetch.OvfetchError("loading libnpu_driver_compiler.so: bad ELF")

    monkeypatch.setattr(tts.ovfetch, "preload", broken_preload)
    if fatal:
        with pytest.raises(tts.ovfetch.OvfetchError, match="bad ELF"):
            tts.Synthesizer(device, fake_runtime)
        assert FakePipeline.instances == []
    else:
        assert tts.Synthesizer(device, fake_runtime).synthesize("Hello").sample_rate == 24000


def test_missing_runtime_dependencies_say_how_to_install_them(fake_runtime, monkeypatch):
    monkeypatch.setitem(sys.modules, "openvino_genai", None)
    with pytest.raises(RuntimeError, match="uv sync"):
        tts.Synthesizer("CPU", fake_runtime)


def test_no_speech_from_the_pipeline_is_an_error(fake_runtime, monkeypatch):
    def silent(self, *args, **kwargs):
        return type("Result", (), {"speeches": [], "output_sample_rate": 24000})()

    monkeypatch.setattr(FakePipeline, "generate", silent)
    with pytest.raises(RuntimeError, match="did not return any speech"):
        tts.Synthesizer("CPU", fake_runtime).synthesize("Hello")


@pytest.fixture
def fake_soundfile(monkeypatch):
    from conftest import FakeSoundFile

    FakeSoundFile.opened.clear()
    soundfile = ModuleType("soundfile")
    soundfile.SoundFile = FakeSoundFile
    monkeypatch.setitem(sys.modules, "soundfile", soundfile)
    return FakeSoundFile.opened


def test_synthesize_writes_a_wav_at_the_models_rate(fake_runtime, fake_soundfile, tmp_path):
    output = tmp_path / "new" / "dir" / "out.wav"

    assert tts.synthesize("Hello", output, device="CPU", model_dir=fake_runtime) == output

    [written] = fake_soundfile
    assert output.read_bytes() == b"WAV/PCM_16"
    assert written.rate == 24000
    np.testing.assert_allclose(np.concatenate(written.writes), [0.1, -0.1])


def test_synthesize_without_soundfile_says_how_to_install_it(monkeypatch, tmp_path):
    monkeypatch.setitem(sys.modules, "soundfile", None)
    with pytest.raises(RuntimeError, match="uv sync"):
        tts.synthesize("Hello", tmp_path / "out.wav", model_dir=tmp_path / "never-created")
    assert not (tmp_path / "never-created").exists()


class FakeSpeech:
    instances: list["FakeSpeech"] = []

    def __init__(
        self,
        audio,
        player=None,
        audio_device=None,
        sample_rate=None,
        on_first_audio=None,
        volume=1.0,
        on_progress=None,
        *,
        interrupt=False,
    ):
        self.volume = volume
        self.chunks = list(audio)
        self.player = player
        self.audio_device = audio_device
        self.sample_rate = sample_rate
        self.interrupt = interrupt
        self.stopped = False
        FakeSpeech.instances.append(self)

    def wait(self):
        if self.interrupt:
            raise KeyboardInterrupt

    def stop(self):
        self.stopped = True


@pytest.mark.parametrize("interrupt", [False, True])
def test_speak_plays_on_the_chosen_device_and_always_stops(fake_runtime, monkeypatch, interrupt):
    FakeSpeech.instances.clear()
    monkeypatch.setattr(tts, "require_player", lambda: "/usr/bin/pw-play")
    monkeypatch.setattr(tts, "Speech", lambda *args: FakeSpeech(*args, interrupt=interrupt))

    if interrupt:
        with pytest.raises(KeyboardInterrupt):
            tts.speak("Hello there.", device="CPU", model_dir=fake_runtime, audio_device="sink")
    else:
        tts.speak("Hello there.", device="CPU", model_dir=fake_runtime, audio_device="sink")

    [speech] = FakeSpeech.instances
    assert speech.stopped
    assert (speech.player, speech.audio_device) == ("/usr/bin/pw-play", "sink")
    assert speech.sample_rate == 24000, "the player starts before the first chunk"
    assert [chunk.sample_rate for chunk in speech.chunks] == [24000]


def test_synthesizer_speak_plays_on_the_given_device(fake_runtime, monkeypatch):
    FakeSpeech.instances.clear()
    monkeypatch.setattr(tts, "Speech", FakeSpeech)
    speech = tts.Synthesizer("CPU", fake_runtime).speak("Hi.", audio_device="sink", volume=0.4)
    assert speech is FakeSpeech.instances[0]
    assert speech.audio_device == "sink"
    assert speech.volume == 0.4
    assert speech.sample_rate == 24000
    assert len(speech.chunks) == 1


def test_a_blend_is_the_weighted_mean_of_its_voices():
    a = np.asarray([1.0, 0.0], dtype=np.float32)
    b = np.asarray([0.0, 1.0], dtype=np.float32)
    mixed = tts.blend([(a, 0.75), (b, 0.25)])
    np.testing.assert_allclose(mixed, [0.75, 0.25])
    assert mixed.dtype == np.float32


def test_a_blend_reaches_the_pipeline_as_one_embedding(fake_runtime):
    np.asarray([4.0, 0.0], dtype=np.float32).tofile(fake_runtime / "voices" / "bm_george.bin")
    synthesizer = tts.Synthesizer("CPU", fake_runtime)

    synthesizer.synthesize("Hello", voice="af_heart(1)+bm_george(1)")

    embedding = next(iter(synthesizer._voices.values()))
    np.testing.assert_allclose(embedding.data, [2.0, 0.5])


def test_a_blend_naming_a_missing_voice_is_refused(fake_runtime):
    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    with pytest.raises(ValueError, match="'bm_nobody' was not found"):
        synthesizer.synthesize("Hello", voice="af_heart+bm_nobody")


def test_voice_blend_cache_is_bounded_as_lru(fake_runtime):
    np.asarray([4.0, 0.0], dtype=np.float32).tofile(fake_runtime / "voices" / "bm_george.bin")
    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    for w in range(1, tts.MAX_CACHED_VOICES + 20):
        synthesizer.synthesize("Hello", voice=f"af_heart({w})+bm_george(1)")
    assert len(synthesizer._voices) == tts.MAX_CACHED_VOICES
    assert tts.voices.canonical("af_heart(1)+bm_george(1)") not in synthesizer._voices
    assert (
        tts.voices.canonical(f"af_heart({tts.MAX_CACHED_VOICES + 19})+bm_george(1)")
        in synthesizer._voices
    )


@pytest.mark.parametrize(
    ("voice", "language", "spoken_as"),
    [("af_heart", None, "en-us"), ("ef_dora", None, "es"), ("ef_dora", "en-gb", "en-gb")],
)
def test_the_language_is_the_voices_own_unless_one_is_asked_for(
    fake_runtime, voice, language, spoken_as
):
    np.arange(2, dtype=np.float32).tofile(fake_runtime / "voices" / "ef_dora.bin")
    synthesizer = tts.Synthesizer("CPU", fake_runtime)

    synthesizer.synthesize("Hola", voice=voice, language=language)

    assert FakePipeline.instances[0].calls[-1][1] == spoken_as


def test_a_pause_renders_as_silence_without_the_pipeline(fake_runtime):
    from vino_speak.script import Step

    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    audio = synthesizer.render(Step("", "af_heart", None, 1.0, silence=0.5))
    assert audio.sample_rate == 24000
    assert len(audio.samples) == 12000 and not audio.samples.any()
    assert FakePipeline.instances[0].calls == []


def test_stream_reads_ssml_and_the_synthesizers_lexicon(fake_runtime):
    from vino_speak.lexicon import Lexicon

    synthesizer = tts.Synthesizer("CPU", fake_runtime, Lexicon({"sql": "sequel"}))
    audio = list(synthesizer.stream('<speak>sql<break time="1s"/>done</speak>'))
    assert [len(a.samples) for a in audio] == [2, 24000, 2]
    assert [call[0] for call in FakePipeline.instances[0].calls] == ["sequel", "done"]


def test_bad_ssml_is_refused_before_the_model_loads(tmp_path):
    with pytest.raises(ValueError, match="well-formed"):
        tts.speak("<speak>open", model_dir=tmp_path / "never-created")
    assert not (tmp_path / "never-created").exists()


class TaggingTranslator:
    def __init__(self):
        self.asked = []

    def stream(self, text, language):
        self.asked.append((text, language))
        yield "Hola."
        yield "Adiós."


def test_a_translation_is_read_a_piece_at_a_time_in_its_language(fake_runtime):
    np.arange(2, dtype=np.float32).tofile(fake_runtime / "voices" / "ef_dora.bin")
    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    translator = TaggingTranslator()

    audio = list(synthesizer.stream("Hello. Bye.", "ef_dora", None, 1.0, "es", translator))

    assert len(audio) == 2
    assert translator.asked == [("Hello. Bye.", "es")]
    assert FakePipeline.instances[0].calls == [("Hola.", "es", 1.0), ("Adiós.", "es", 1.0)]


def test_translate_to_needs_a_translator(fake_runtime):
    with pytest.raises(ValueError, match="needs a Translator"):
        tts.Synthesizer("CPU", fake_runtime).stream("Hi.", translate_to="es")


@pytest.mark.parametrize(
    ("text", "target", "complaint"),
    [("<speak>Hi.</speak>", "es", "SSML can't be translated"), ("Hi.", "hi", "can't translate")],
)
def test_what_cant_be_translated_is_refused_before_the_model_loads(
    tmp_path, text, target, complaint
):
    with pytest.raises(ValueError, match=complaint):
        tts.speak(
            text, translate_to=target, translator=TaggingTranslator(), model_dir=tmp_path / "x"
        )
    assert not (tmp_path / "x").exists()


def test_a_trailing_symbol_kokoro_cant_end_on_is_not_sent(fake_runtime):
    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    synthesizer.synthesize("Hola amigos.»", language="es")
    assert synthesizer.synthesize("—", language="es").samples.size == 0
    assert FakePipeline.instances[0].calls == [("Hola amigos.", "es", 1.0)]


def test_each_chunk_says_how_far_through_the_text_it_is(fake_runtime):
    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    text = "First. " + "This second sentence is long enough to be a chunk of its own here. " * 3
    progress = [audio.progress for audio in synthesizer.stream(text)]
    assert progress[0][0] == 0.0
    assert progress[-1][1] == pytest.approx(1.0)
    assert all(a[1] == pytest.approx(b[0]) for a, b in zip(progress, progress[1:], strict=False))


class Pieces:
    """Translates each piece of `chunks(text, MAX_PIECE_CHARS)` to itself,
    the second to nothing."""

    def stream(self, text, language):
        from vino_speak.text import chunks
        from vino_speak.translate import MAX_PIECE_CHARS

        for i, piece in enumerate(chunks(text, MAX_PIECE_CHARS)):
            yield "" if i == 1 else piece


def test_a_translated_chunk_takes_its_source_pieces_share(fake_runtime):
    np.arange(2, dtype=np.float32).tofile(fake_runtime / "voices" / "ef_dora.bin")
    synthesizer = tts.Synthesizer("CPU", fake_runtime)
    text = "Uno. " + "Dos tres cuatro cinco seis siete ocho nueve diez once doce. " * 40
    progress = [
        a.progress
        for a in synthesizer.stream(text, "ef_dora", translate_to="es", translator=Pieces())
    ]
    assert progress[0][0] == 0.0
    assert progress[-1][1] == pytest.approx(1.0), "the last piece ends the text"
    from vino_speak.text import chunks
    from vino_speak.translate import MAX_PIECE_CHARS

    sources = [len(c) for c in chunks(text, MAX_PIECE_CHARS)]
    gaps = [b[0] - a[1] for a, b in zip(progress, progress[1:], strict=False)]
    skipped = sources[1] / sum(sources)
    assert max(gaps) == pytest.approx(skipped), "the untranslated piece's share is skipped"
