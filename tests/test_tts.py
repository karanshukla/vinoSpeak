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
    writes = []
    soundfile = ModuleType("soundfile")
    soundfile.write = lambda path, samples, rate: writes.append((path, samples.copy(), rate))
    monkeypatch.setitem(sys.modules, "soundfile", soundfile)
    return writes


def test_synthesize_writes_a_wav_at_the_models_rate(fake_runtime, fake_soundfile, tmp_path):
    output = tmp_path / "new" / "dir" / "out.wav"

    assert tts.synthesize("Hello", output, device="CPU", model_dir=fake_runtime) == output

    assert output.parent.is_dir()
    [(path, samples, rate)] = fake_soundfile
    assert path == output
    assert rate == 24000
    np.testing.assert_allclose(samples, [0.1, -0.1])


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
        *,
        interrupt=False,
    ):
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
    speech = tts.Synthesizer("CPU", fake_runtime).speak("Hi.", audio_device="sink")
    assert speech is FakeSpeech.instances[0]
    assert speech.audio_device == "sink"
    assert speech.sample_rate == 24000
    assert len(speech.chunks) == 1
