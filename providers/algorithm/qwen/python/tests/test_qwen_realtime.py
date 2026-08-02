import importlib.util
import os
import pathlib
import ssl
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
path = pathlib.Path(__file__).parents[1] / "nodes/qwen_realtime/node.py"
spec = importlib.util.spec_from_file_location("qwen_node", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class FakeTransport:
    def __init__(self, events):
        self.events, self.sent, self.closed = events, [], False

    def send(self, event): self.sent.append(event)
    def poll(self): return iter(self.events)
    def close(self): self.closed = True


class Context:
    def __init__(self): self.emissions, self.signals, self.events = [], [], []
    def emit(self, port, frame): self.emissions.append((port, frame))
    def emit_signal(self, name, payload): self.signals.append((name, payload))
    def publish_event(self, topic, payload): self.events.append((topic, payload))


class QwenNodeTests(unittest.TestCase):
    def test_nonblocking_ssl_want_read_is_not_a_runtime_failure(self):
        class Socket:
            def recv(self): raise ssl.SSLWantReadError()

        transport = module._QwenWebSocket.__new__(module._QwenWebSocket)
        transport._socket = Socket()
        transport._websocket = types.SimpleNamespace(WebSocketTimeoutException=TimeoutError)
        self.assertEqual(list(transport.poll()), [])

    def test_protocol_and_barge_in_without_credentials_or_network(self):
        transport = FakeTransport([
            {"type": "response.created"},
            {"type": "input_audio_buffer.speech_started"},
            {"type": "conversation.item.input_audio_transcription.delta", "text": "用户说"},
            {"type": "response.audio_transcript.delta", "delta": "你好"},
            {"type": "response.audio.delta", "delta": "AQIDBA=="},
            {"type": "response.done"},
        ])
        node = module.QwenAudioRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        ctx = Context()
        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=7), ctx)
        self.assertEqual(transport.sent[0]["type"], "input_audio_buffer.append")
        self.assertEqual(transport.sent[1]["type"], "response.cancel")
        self.assertEqual(ctx.signals[0][0], "voxa.runtime.interrupt")
        self.assertEqual(ctx.emissions[0][1].text, "用户说")
        self.assertEqual(ctx.emissions[1][1].text, "你好")
        self.assertEqual(ctx.emissions[2][1].sample_rate_hz, 24000)

    def test_session_update_contains_no_credentials(self):
        update = module.session_update({})
        rendered = str(update)
        self.assertNotIn("secret", rendered)
        self.assertIn("smart_turn", rendered)
        self.assertEqual(update["session"]["input_audio_format"], "pcm")
        self.assertEqual(update["session"]["output_audio_format"], "pcm")
        self.assertEqual(update["session"]["voice"], "longanqian")
        self.assertNotIn("input_audio_transcription", update["session"])

    def test_idle_speech_interrupts_voxa_without_invalid_provider_cancel(self):
        transport = FakeTransport([{"type": "input_audio_buffer.speech_started"}])
        node = module.QwenAudioRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        ctx = Context()
        node.on_process(AudioFrame(b"\0" * 640, 16000), ctx)
        self.assertEqual([item["type"] for item in transport.sent], ["input_audio_buffer.append"])
        self.assertEqual(ctx.signals[0][0], "voxa.runtime.interrupt")


if __name__ == "__main__":
    unittest.main()
