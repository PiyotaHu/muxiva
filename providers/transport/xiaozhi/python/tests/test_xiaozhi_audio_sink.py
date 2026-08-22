from __future__ import annotations

import importlib.util
import pathlib
import sys
import types
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).parents[1]
sys.path.insert(0, str(ROOT))
NODE_PATH = ROOT / "nodes" / "xiaozhi_audio_sink" / "node.py"


class FakeClient:
    instances = []

    def __init__(self, *_args, **_kwargs):
        self.connected = True
        self.sent = []
        self.__class__.instances.append(self)

    def connect(self):
        self.connected = True
        return True

    def is_connected(self):
        return self.connected

    def send(self, value):
        self.sent.append(value)

    def close(self):
        self.connected = False


class Frame:
    def __init__(self, data: bytes, sequence: int):
        self.data = data
        self.sequence = sequence


class Signal:
    def __init__(self, sequence: int):
        self.sequence = sequence


class XiaozhiAudioSinkTests(unittest.TestCase):
    def setUp(self):
        FakeClient.instances.clear()
        spec = importlib.util.spec_from_file_location("xiaozhi_audio_sink_test_node", NODE_PATH)
        self.module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        fake_gateway = types.SimpleNamespace(XiaozhiControlClient=FakeClient)
        with mock.patch.dict(sys.modules, {"xiaozhi_gateway": fake_gateway}):
            spec.loader.exec_module(self.module)

    def test_signal_clears_gateway_and_drops_late_old_pcm(self):
        node = self.module.XiaozhiAudioSinkNode()
        client = FakeClient.instances[-1]

        node.on_process(Frame(b"old-before", 100), None)
        node.on_signal(Signal(200))
        node.on_process(Frame(b"old-after-reset", 100), None)
        node.on_process(Frame(b"current-turn", 200), None)

        self.assertEqual(client.sent, [
            {"op": "audio", "pcm_hex": b"old-before".hex()},
            {"op": "reset"},
            {"op": "audio", "pcm_hex": b"current-turn".hex()},
        ])


if __name__ == "__main__":
    unittest.main()
