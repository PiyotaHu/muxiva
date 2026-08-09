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


shim = types.ModuleType("muxiva")
shim.AudioFrame, shim.TextFrame, shim.EventFrame = AudioFrame, TextFrame, EventFrame
sys.modules["muxiva"] = shim
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
    def __init__(self):
        self.emissions, self.signals, self.events = [], [], []
        self.counters, self.gauges = {}, {}
    def emit(self, port, frame): self.emissions.append((port, frame))
    def emit_signal(self, name, payload): self.signals.append((name, payload))
    def publish_notification(self, topic, payload): self.events.append((topic, payload))
    def increment_counter(self, name, amount=1):
        self.counters[name] = self.counters.get(name, 0) + amount
    def set_gauge(self, name, value): self.gauges[name] = value


class QwenNodeTests(unittest.TestCase):
    def test_transport_waits_for_session_configuration_before_accepting_audio(self):
        actions = []

        class Socket:
            def __init__(self):
                self.events = iter([
                    '{"type":"session.created"}',
                    '{"type":"session.updated"}',
                ])
            def recv(self):
                actions.append("recv")
                return next(self.events)
            def send(self, value):
                actions.append(("send", value))
            def settimeout(self, value):
                actions.append(("timeout", value))
            def close(self):
                actions.append("close")

        socket = Socket()
        websocket = types.SimpleNamespace(
            create_connection=lambda *_args, **_kwargs: socket,
            WebSocketTimeoutException=TimeoutError,
        )
        with mock.patch.dict(sys.modules, {"websocket": websocket}):
            transport = module._QwenWebSocket(
                "wss://example.invalid", "secret", {"type": "session.update"}
            )

        self.assertEqual(actions[0], "recv")
        self.assertEqual(actions[1][0], "send")
        self.assertEqual(actions[2], "recv")
        self.assertEqual(actions[3], ("timeout", 0))
        self.assertEqual(
            [event["type"] for event in transport.poll(maximum=2)],
            ["session.created", "session.updated"],
        )

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
        node = module.QwenAudioRealtimeNode({"input_chunk_ms": 20}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        ctx = Context()
        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=7), ctx)
        self.assertEqual(transport.sent[0]["type"], "input_audio_buffer.append")
        self.assertEqual(transport.sent[1]["type"], "response.cancel")
        self.assertEqual(ctx.signals[0][0], "muxiva.voice.speech.started")
        self.assertIn(
            ("muxiva.voice.barge_in", {"node": "qwen.audio_realtime", "response_cancelled": True}),
            ctx.events,
        )
        ports = [port for port, _ in ctx.emissions]
        self.assertNotIn("response_text_out", ports)
        self.assertNotIn("audio_out", ports, "late output from the cancelled response is discarded")
        self.assertIn("transcript_preview_out", ports)
        self.assertIn("transcript_out", ports)
        self.assertIn("event_out", ports)
        self.assertNotIn("client_event_out", ports)
        self.assertIn(("muxiva.voice.transcript.preview", {"text": "用户说"}), ctx.events)
        self.assertIn(("muxiva.voice.transcript.completed", {"text": "用户说"}), ctx.events)
        self.assertEqual(ctx.counters["qwen.audio_chunks_sent"], 1)

    def test_uncancelled_response_emits_text_audio_and_completion(self):
        transport = FakeTransport([
            {"type": "response.created"},
            {"type": "response.audio_transcript.delta", "delta": "你好"},
            {"type": "response.audio.delta", "delta": "AQIDBA=="},
            {"type": "response.done"},
        ])
        node = module.QwenAudioRealtimeNode({"input_chunk_ms": 20}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        ctx = Context()
        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=8), ctx)
        ports = [port for port, _ in ctx.emissions]
        self.assertIn("response_text_out", ports)
        self.assertIn("audio_out", ports)
        self.assertIn("event_out", ports)
        self.assertNotIn("client_event_out", ports)
        self.assertIn(
            ("muxiva.voice.response.completed", {"text": "你好", "audio_bytes": 4}),
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

    def test_default_batches_ten_millisecond_frames_into_recommended_chunk(self):
        transport = FakeTransport([])
        node = module.QwenAudioRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        ctx = Context()
        for sequence in range(9):
            node.on_process(AudioFrame(b"\x00\x01" * 160, 16000, sequence=sequence), ctx)
        self.assertEqual(transport.sent, [])
        node.on_process(AudioFrame(b"\x00\x01" * 160, 16000, sequence=9), ctx)
        self.assertEqual(len(transport.sent), 1)
        self.assertEqual(transport.sent[0]["type"], "input_audio_buffer.append")
        self.assertEqual(ctx.counters["input.audio_frames"], 10)
        self.assertEqual(ctx.counters["qwen.audio_chunks_sent"], 1)
        self.assertEqual(ctx.gauges["input.audio_peak_pcm16"], 256)

    def test_smart_turn_does_not_include_server_vad_tuning(self):
        detection = module.session_update({"turn_detection": "smart_turn"})["session"]["turn_detection"]
        self.assertEqual(detection, {"type": "smart_turn"})

    def test_idle_speech_interrupts_muxiva_without_invalid_provider_cancel(self):
        transport = FakeTransport([{"type": "input_audio_buffer.speech_started"}])
        node = module.QwenAudioRealtimeNode({"input_chunk_ms": 20}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        ctx = Context()
        node.on_process(AudioFrame(b"\0" * 640, 16000), ctx)
        self.assertEqual([item["type"] for item in transport.sent], ["input_audio_buffer.append"])
        self.assertEqual(ctx.signals[0][0], "muxiva.voice.speech.started")
        self.assertFalse(any(topic == "muxiva.voice.barge_in" for topic, _ in ctx.events))

    def test_real_qwen_late_cancel_race_recovers_and_answers_the_next_turn(self):
        transport = FakeTransport([
            {"type": "response.created"},
            {"type": "input_audio_buffer.speech_started"},
            {"type": "response.done"},
            {
                "type": "error",
                "error": {
                    "code": "invalid_value",
                    "message": "Conversation has no active response.",
                },
            },
            {"type": "input_audio_buffer.speech_stopped"},
            {"type": "response.created"},
            {
                "type": "conversation.item.input_audio_transcription.completed",
                "transcript": "猴子在树上扒得紧不紧，会不会掉下来？",
            },
            {"type": "response.audio_transcript.delta", "delta": "抓得牢通常不会掉。"},
            {"type": "response.audio.delta", "delta": "AQIDBA=="},
            {"type": "response.done"},
        ])
        node = module.QwenAudioRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        ctx = Context()
        node.on_process(AudioFrame(b"\0" * 640, 16000), ctx)

        self.assertEqual(
            [item["type"] for item in transport.sent],
            ["response.cancel"],
        )
        self.assertIn(
            ("muxiva.voice.transcript.completed", {
                "text": "猴子在树上扒得紧不紧，会不会掉下来？"
            }),
            ctx.events,
        )
        self.assertIn("response_text_out", [port for port, _ in ctx.emissions])
        self.assertIn("audio_out", [port for port, _ in ctx.emissions])
        self.assertIn(
            ("muxiva.voice.response.completed", {
                "text": "抓得牢通常不会掉。", "audio_bytes": 4
            }),
            ctx.events,
        )

    def test_no_active_response_error_without_local_cancel_is_fatal(self):
        transport = FakeTransport([{
            "type": "error",
            "error": {
                "code": "invalid_value",
                "message": "Conversation has no active response.",
            },
        }])
        node = module.QwenAudioRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, {
            "DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace"
        }):
            node.on_prepare()
        with self.assertRaises(module.QwenProtocolError):
            node.on_process(AudioFrame(b"\0" * 640, 16000), Context())


if __name__ == "__main__":
    unittest.main()
