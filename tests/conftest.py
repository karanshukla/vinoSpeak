import numpy as np


class FakeTensor:
    def __init__(self, *args):
        if len(args) == 1:
            self.data = args[0]
        else:
            self.element_type, self.shape = args
            self.data = np.zeros(self.shape, dtype=np.float32)


class FakeOpenVINO:
    Tensor = FakeTensor
