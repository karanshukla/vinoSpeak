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
