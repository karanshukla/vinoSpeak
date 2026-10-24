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
