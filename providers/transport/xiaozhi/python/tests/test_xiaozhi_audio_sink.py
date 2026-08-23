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
    def __init__(self, sequence: int, name: str = "muxiva.turn.cancelled"):
        self.name = name
        self.sequence = sequence


class Event:
    def __init__(self, sequence: int, audio_frames: int):
        self.topic = "muxiva.voice.tts.drained"
        self.sequence = sequence
        self.payload = '{"audio_frames":%d}' % audio_frames


class Context:
    def __init__(self, input_port: str):
        self.input_port = input_port


class XiaozhiAudioSinkTests(unittest.TestCase):
    def setUp(self):
        FakeClient.instances.clear()
        spec = importlib.util.spec_from_file_location("xiaozhi_audio_sink_test_node", NODE_PATH)
        self.module = importlib.util.module_from_spec(spec)
        assert spec.loader is not None
        fake_gateway = types.SimpleNamespace(
            XiaozhiControlClient=FakeClient,
            TTS_TYPE="tts",
        )
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

    def test_tts_stop_waits_until_every_declared_audio_frame_reaches_sink(self):
        node = self.module.XiaozhiAudioSinkNode()
        client = FakeClient.instances[-1]

        node.on_process(Frame(b"first", 404), Context("audio_in"))
        node.on_process(Event(404, 3), Context("event_in"))
        self.assertFalse(any(command.get("op") == "message" for command in client.sent))

        node.on_process(Frame(b"second", 404), Context("audio_in"))
        self.assertFalse(any(command.get("op") == "message" for command in client.sent))

        node.on_process(Frame(b"tail", 404), Context("audio_in"))
        self.assertEqual(client.sent[-1], {
            "op": "message",
            "payload": {"type": "tts", "state": "stop"},
        })

    def test_tts_stop_is_immediate_when_audio_arrived_before_drain_event(self):
        node = self.module.XiaozhiAudioSinkNode()
        client = FakeClient.instances[-1]

        node.on_process(Frame(b"first", 405), Context("audio_in"))
        node.on_process(Frame(b"tail", 405), Context("audio_in"))
        node.on_process(Event(405, 2), Context("event_in"))

        self.assertEqual(client.sent[-1]["payload"], {"type": "tts", "state": "stop"})

    def test_barge_in_during_media_barrier_clears_tail_and_stops_device(self):
        node = self.module.XiaozhiAudioSinkNode()
        client = FakeClient.instances[-1]

        node.on_process(Frame(b"first", 500), Context("audio_in"))
        node.on_process(Event(500, 3), Context("event_in"))
        node.on_signal(Signal(600))
        node.on_process(Frame(b"late-tail", 500), Context("audio_in"))

        self.assertEqual(client.sent[-2:], [
            {"op": "reset"},
            {"op": "message", "payload": {"type": "tts", "state": "stop"}},
        ])
        self.assertFalse(any(
            command.get("pcm_hex") == b"late-tail".hex()
            for command in client.sent
        ))


if __name__ == "__main__":
    unittest.main()
