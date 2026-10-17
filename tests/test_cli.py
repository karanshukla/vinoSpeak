import tempfile
import unittest
from pathlib import Path
from types import ModuleType
from unittest.mock import Mock, patch

import numpy as np

from vino_speak.cli import (
    MODEL_FILES,
    MODEL_ID,
    build_parser,
    ensure_model,
    get_audio_data,
    load_voice_embedding,
    model_is_ready,
    validate_inputs,
)


class FakeTensor:
    def __init__(self, *args):
        if len(args) == 1:
            self.data = args[0]
        else:
            self.element_type, self.shape = args
            self.data = np.zeros(self.shape, dtype=np.float32)


class FakeOpenVINO:
    Tensor = FakeTensor


class FakeRemoteSpeech:
    element_type = "f32"
    shape = (2,)

    @property
    def data(self):
        raise RuntimeError("Remote tensor is not host-accessible")

    def copy_to(self, host_tensor):
        host_tensor.data[:] = (0.25, -0.25)


class TtsCliTests(unittest.TestCase):
    def test_parser_defaults_to_npu(self):
        arguments = build_parser().parse_args(["Hello"])
        self.assertEqual(arguments.device, "NPU")
        self.assertEqual(arguments.voice, "af_heart")

    def test_validate_inputs_rejects_blank_text_unsafe_voice_and_bad_speed(self):
        for text, voice, speed in (
            ("  ", "af_heart", 1.0),
            ("Hello", "../voice", 1.0),
            ("Hello", "af_heart", 2.1),
        ):
            with self.subTest(text=text, voice=voice, speed=speed):
                with self.assertRaises(ValueError):
                    validate_inputs(text, voice, speed)

    def test_load_voice_embedding_checks_size_and_shape(self):
        with tempfile.TemporaryDirectory() as temporary_directory:
            voice_path = Path(temporary_directory) / "voice.bin"
            np.arange(4, dtype=np.float32).tofile(voice_path)

            embedding = load_voice_embedding(voice_path, (2, 2), FakeOpenVINO)

            np.testing.assert_array_equal(embedding.data, [[0, 1], [2, 3]])
            with self.assertRaisesRegex(ValueError, "expects 3"):
                load_voice_embedding(voice_path, (3,), FakeOpenVINO)

    def test_audio_data_copies_remote_tensor_to_host(self):
        audio = get_audio_data(FakeRemoteSpeech(), FakeOpenVINO)

        np.testing.assert_array_equal(audio, [0.25, -0.25])

    def test_audio_data_rejects_empty_or_non_finite_waveforms(self):
        for values in ([], [float("nan")]):
            with self.subTest(values=values):
                with self.assertRaisesRegex(RuntimeError, "empty or contains non-finite"):
                    get_audio_data(FakeTensor(np.asarray(values)), FakeOpenVINO)

    def test_model_readiness_requires_all_pipeline_assets(self):
        with tempfile.TemporaryDirectory() as temporary_directory:
            model_dir = Path(temporary_directory)
            self.assertFalse(model_is_ready(model_dir))
            for filename in MODEL_FILES:
                asset = model_dir / filename
                asset.parent.mkdir(parents=True, exist_ok=True)
                asset.touch()
            self.assertTrue(model_is_ready(model_dir))

    def test_ensure_model_downloads_the_int8_model_when_missing(self):
        with tempfile.TemporaryDirectory() as temporary_directory:
            model_dir = Path(temporary_directory) / "kokoro"

            def populate_model(repo_id, local_dir):
                for filename in MODEL_FILES:
                    asset = Path(local_dir) / filename
                    asset.parent.mkdir(parents=True, exist_ok=True)
                    asset.touch()

            snapshot_download = Mock(side_effect=populate_model)
            hub = ModuleType("huggingface_hub")
            hub.snapshot_download = snapshot_download

            with patch.dict("sys.modules", {"huggingface_hub": hub}):
                result = ensure_model(model_dir)

            self.assertEqual(result, model_dir)
            snapshot_download.assert_called_once_with(
                repo_id=MODEL_ID, local_dir=str(model_dir)
            )


if __name__ == "__main__":
    unittest.main()
