import numpy as np
import pytest

from vino_speak import ovfetch


@pytest.fixture(autouse=True)
def no_real_preload(monkeypatch):
    """A developer's ~/.local/lib/ovfetch-npu must not be dlopen()ed by a
    test; the preload tests reset this to None themselves."""
    monkeypatch.setattr(ovfetch, "_preloaded", [])


class FakeTensor:
    def __init__(self, *args):
        if len(args) == 1:
            self.data = args[0]
        else:
            self.element_type, self.shape = args
            self.data = np.zeros(self.shape, dtype=np.float32)


class FakeOpenVINO:
    Tensor = FakeTensor


class FakeSoundFile:
    """soundfile.SoundFile for writing: keeps what was written and leaves
    the format's name in the file when it closes."""

    opened: list["FakeSoundFile"] = []

    def __init__(self, path, mode, samplerate, channels, subtype, format):
        assert mode == "w" and channels == 1
        self.path, self.rate, self.subtype, self.format = path, samplerate, subtype, format
        self.writes = []
        FakeSoundFile.opened.append(self)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        from pathlib import Path

        Path(self.path).write_bytes(f"{self.format}/{self.subtype}".encode())

    def write(self, samples):
        self.writes.append(np.array(samples))
