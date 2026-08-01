import importlib.util
import os
import pathlib
import sys
import types
import unittest
from unittest import mock


class AudioFrame:
    def __init__(self, data, sample_rate_hz, channels=1, sequence=0):
        self.data, self.sample_rate_hz = data, sample_rate_hz
        self.channels, self.sequence = channels, sequence


class TextFrame:
    def __init__(self, text, sequence=0):
        self.text, self.sequence = text, sequence


shim = types.ModuleType("voxa")
shim.AudioFrame, shim.TextFrame = AudioFrame, TextFrame
sys.modules["voxa"] = shim
root = pathlib.Path(__file__).parents[1] / ".voxa/nodes"


def load(package):
    spec = importlib.util.spec_from_file_location(package, root / package / "node.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


asr, llm, tts = (load(name) for name in (
    "qwen_asr_realtime", "qwen_llm_stream", "qwen_tts_realtime"
))


class FakeTransport:
    def __init__(self, events=()):
        self.events, self.sent, self.closed = list(events), [], False

    def send(self, event): self.sent.append(event)
    def poll(self): return iter(self.events)
    def close(self): self.closed = True


class Context:
    def __init__(self): self.emissions, self.events = [], []
    def emit(self, port, frame): self.emissions.append((port, frame))
    def publish_event(self, topic, payload): self.events.append((topic, payload))


class CascadeNodeTests(unittest.TestCase):
    credentials = {"DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace-1"}

    def test_asr_emits_only_completed_transcript(self):
        transport = FakeTransport([
            {"type": "conversation.item.input_audio_transcription.delta", "text": "你"},
            {"type": "conversation.item.input_audio_transcription.completed", "transcript": "你好 Voxa"},
        ])
        node = asr.QwenAsrRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, self.credentials): node.on_prepare()
        ctx = Context()
        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=9), ctx)
        self.assertEqual(transport.sent[0]["type"], "input_audio_buffer.append")
        self.assertEqual(ctx.emissions[0][1].text, "你好 Voxa")
        self.assertEqual(ctx.events[0][0], "voxa.voice.transcript.completed")

    def test_llm_forwards_each_sse_delta_through_ctx(self):
        class Client:
            def stream(self, endpoint, key, payload):
                self.request = endpoint, key, payload
                return iter(["你好", "，", "我是 Voxa。"])
        client = Client()
        node = llm.QwenLlmStreamNode({}, lambda: client)
        ctx = Context()
        with mock.patch.dict(os.environ, self.credentials):
            node.on_process(TextFrame("你是谁？", sequence=3), ctx)
        self.assertEqual([frame.text for _, frame in ctx.emissions], ["你好，我是 Voxa。"])
        self.assertTrue(client.request[2]["stream"])
        self.assertEqual(ctx.events[0][0], "voxa.voice.response.delta")
        self.assertEqual(ctx.events[-1][0], "voxa.voice.response.completed")

    def test_tts_converts_audio_delta_to_24k_pcm(self):
        transport = FakeTransport([
            {"type": "response.audio.delta", "delta": "AQIDBA=="},
            {"type": "response.done"},
        ])
        node = tts.QwenTtsRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, self.credentials): node.on_prepare()
        ctx = Context()
        node.on_process(TextFrame("你好", sequence=4), ctx)
        self.assertEqual(transport.sent[0]["type"], "input_text_buffer.append")
        self.assertEqual(transport.sent[1]["type"], "input_text_buffer.commit")
        self.assertEqual(ctx.emissions[0][1].sample_rate_hz, 24000)
        self.assertEqual(ctx.emissions[0][1].data, b"\x01\x02\x03\x04")

    def test_manifests_share_an_identical_connection_contract(self):
        import json
        connections = []
        for package in ("qwen_realtime", "qwen_asr_realtime", "qwen_llm_stream", "qwen_tts_realtime"):
            connections.append(json.loads((root / package / "voxa.node.json").read_text())["connection"])
        self.assertTrue(all(connection == connections[0] for connection in connections[1:]))


if __name__ == "__main__":
    unittest.main()
