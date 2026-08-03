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


class EventFrame:
    def __init__(self, topic, payload="", source="python.node", schema_version=1, sequence=0, **_):
        self.topic, self.payload, self.source = topic, payload, source
        self.schema_version, self.sequence = schema_version, sequence


shim = types.ModuleType("voxa")
shim.AudioFrame, shim.TextFrame, shim.EventFrame = AudioFrame, TextFrame, EventFrame
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
            {"type": "conversation.item.input_audio_transcription.delta", "text": "用户", "stash": "说"},
            {"type": "conversation.item.input_audio_transcription.completed", "transcript": "用户说"},
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
        self.assertEqual(ctx.signals[0][0], "voxa.voice.speech.started")
        self.assertIn(
            ("voxa.voice.barge_in", {"node": "qwen.audio_realtime", "response_cancelled": True}),
            ctx.events,
        )
        ports = [port for port, _ in ctx.emissions]
        self.assertNotIn("response_text_out", ports)
        self.assertNotIn("audio_out", ports, "late output from the cancelled response is discarded")
        self.assertIn("transcript_out", ports)
        self.assertIn("client_event_out", ports)
        self.assertIn(("voxa.voice.transcript.preview", {"text": "用户说"}), ctx.events)
        self.assertIn(("voxa.voice.transcript.completed", {"text": "用户说"}), ctx.events)

    def test_uncancelled_response_emits_text_audio_and_completion(self):
        transport = FakeTransport([
            {"type": "response.created"},
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
        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=8), ctx)
        ports = [port for port, _ in ctx.emissions]
        self.assertIn("response_text_out", ports)
        self.assertIn("audio_out", ports)
        self.assertIn("client_event_out", ports)
        self.assertIn(
            ("voxa.voice.response.completed", {"text": "你好", "audio_bytes": 4}),
            ctx.events,
        )

    def test_session_update_contains_no_credentials(self):
        update = module.session_update({})
        rendered = str(update)
        self.assertNotIn("secret", rendered)
        self.assertIn("server_vad", rendered)
        self.assertEqual(update["session"]["turn_detection"]["threshold"], 0.35)
        self.assertEqual(update["session"]["turn_detection"]["silence_duration_ms"], 1000)
        self.assertEqual(update["session"]["input_audio_format"], "pcm")
        self.assertEqual(update["session"]["output_audio_format"], "pcm")
        self.assertEqual(update["session"]["voice"], "longanqian")
        self.assertNotIn("input_audio_transcription", update["session"])

    def test_smart_turn_does_not_include_server_vad_tuning(self):
        detection = module.session_update({"turn_detection": "smart_turn"})["session"]["turn_detection"]
        self.assertEqual(detection, {"type": "smart_turn"})

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
        self.assertEqual(ctx.signals[0][0], "voxa.voice.speech.started")
        self.assertFalse(any(topic == "voxa.voice.barge_in" for topic, _ in ctx.events))

    def test_late_cancel_error_does_not_abort_the_next_turn(self):
        transport = FakeTransport([
            {"type": "response.created"},
            {"type": "input_audio_buffer.speech_started"},
            {"type": "response.done"},
            {
                "type": "error",
                "error": {
                    "code": "invalid_request_error",
                    "message": "Cannot cancel: no active response",
                },
            },
        ])
        node = module.QwenAudioRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        node.on_process(AudioFrame(b"\0" * 640, 16000), Context())


if __name__ == "__main__":
    unittest.main()
