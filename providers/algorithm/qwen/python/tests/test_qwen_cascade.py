import importlib.util
import json
import os
import pathlib
import sys
import threading
import time
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


class SignalFrame:
    def __init__(self, name, sequence=0):
        self.name, self.sequence = name, sequence


shim = types.ModuleType("muxiva")
shim.AudioFrame, shim.TextFrame, shim.EventFrame = AudioFrame, TextFrame, EventFrame
sys.modules["muxiva"] = shim
root = pathlib.Path(__file__).parents[1] / "nodes"


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

    def send(self, event):
        self.sent.append(event)

    def poll(self):
        return iter(self.events)

    def close(self):
        self.closed = True


class Context:
    def __init__(self, input_port=None):
        self.input_port = input_port
        self.emissions, self.signals, self.events, self.scheduled_ticks = [], [], [], []

    def emit(self, port, frame):
        self.emissions.append((port, frame))

    def emit_signal(self, name, payload):
        self.signals.append((name, payload))

    def publish_notification(self, topic, payload):
        self.events.append((topic, payload))

    def schedule_next_tick(self, delay_ms):
        self.scheduled_ticks.append(delay_ms)


def wait_until(predicate, timeout=1.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(0.005)
    return predicate()


class CascadeNodeTests(unittest.TestCase):
    credentials = {"DASHSCOPE_API_KEY": "secret", "DASHSCOPE_WORKSPACE_ID": "workspace-1"}

    def test_asr_default_vad_threshold_is_demo_safe_and_manifest_visible(self):
        sessions = []

        def connect(_endpoint, _key, session):
            sessions.append(session)
            return FakeTransport()

        node = asr.QwenAsrRealtimeNode({}, connect)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()
        self.assertEqual(sessions[0]["session"]["turn_detection"]["threshold"], 0.45)
        manifest = json.loads((root / "qwen_asr_realtime" / "muxiva.node.json").read_text())
        threshold = manifest["config_schema"]["properties"]["vad_threshold"]
        self.assertEqual(threshold["default"], 0.45)
        self.assertIn("Studio", threshold["description"])

    def test_asr_server_vad_emits_speech_signal_state_and_completed_transcript(self):
        transport = FakeTransport([
            {"type": "input_audio_buffer.speech_started"},
            {"type": "conversation.item.input_audio_transcription.text", "text": "你", "stash": "好"},
            {"type": "input_audio_buffer.speech_stopped"},
            {"type": "conversation.item.input_audio_transcription.completed", "transcript": "你好 Muxiva"},
        ])
        node = asr.QwenAsrRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()
        ctx = Context("audio_in")
        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=9), ctx)
        self.assertEqual(transport.sent[0]["type"], "input_audio_buffer.append")
        self.assertEqual(ctx.signals[0][0], "muxiva.voice.speech.started")
        self.assertEqual(
            [frame.topic for port, frame in ctx.emissions if port == "speech_out"],
            ["muxiva.voice.speech.started", "muxiva.voice.speech.stopped"],
        )
        self.assertEqual(
            [frame.text for port, frame in ctx.emissions if port == "text_out"],
            ["你好 Muxiva"],
        )
        self.assertEqual(
            [frame.text for port, frame in ctx.emissions if port == "transcript_preview_out"],
            ["你好"],
        )
        self.assertFalse(any(port == "client_event_out" for port, _ in ctx.emissions))
        self.assertIn(("muxiva.voice.transcript.preview", {"text": "你好"}), ctx.events)
        self.assertIn(
            ("muxiva.voice.transcript.completed", {"text": "你好 Muxiva"}),
            ctx.events,
        )

    def test_asr_drops_late_preview_after_completion_and_reopens_on_next_speech(self):
        transport = FakeTransport([
            {"type": "input_audio_buffer.speech_started"},
            {"type": "conversation.item.input_audio_transcription.text", "text": "第一轮预览"},
            {"type": "input_audio_buffer.speech_stopped"},
            {"type": "conversation.item.input_audio_transcription.completed", "transcript": "第一轮最终文本"},
            # Qwen can deliver this buffered preview after the final transcript.
            {"type": "conversation.item.input_audio_transcription.text", "text": "第一轮最终文本"},
            {"type": "input_audio_buffer.speech_started"},
            {"type": "conversation.item.input_audio_transcription.text", "text": "第二轮预览"},
        ])
        node = asr.QwenAsrRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()
        ctx = Context("audio_in")

        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=1379), ctx)

        self.assertEqual(
            [frame.text for port, frame in ctx.emissions if port == "transcript_preview_out"],
            ["第一轮预览", "第二轮预览"],
        )
        self.assertEqual(
            [frame.text for port, frame in ctx.emissions if port == "text_out"],
            ["第一轮最终文本"],
        )
        self.assertEqual(
            [payload["text"] for topic, payload in ctx.events
             if topic == "muxiva.voice.transcript.preview"],
            ["第一轮预览", "第二轮预览"],
        )

    def test_asr_holds_completed_text_until_server_vad_stops_the_turn(self):
        transport = FakeTransport([
            {"type": "input_audio_buffer.speech_started"},
            {"type": "conversation.item.input_audio_transcription.completed", "transcript": "完整问题"},
            {"type": "input_audio_buffer.speech_stopped"},
        ])
        node = asr.QwenAsrRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()
        ctx = Context("audio_in")

        node.on_process(AudioFrame(b"\0" * 640, 16000, sequence=55), ctx)

        self.assertEqual(
            [(port, getattr(frame, "topic", getattr(frame, "text", "")))
             for port, frame in ctx.emissions],
            [
                ("speech_out", "muxiva.voice.speech.started"),
                ("speech_out", "muxiva.voice.speech.stopped"),
                ("text_out", "完整问题"),
            ],
        )

    def test_llm_background_stream_drains_sentence_and_preserves_sequence(self):
        class Client:
            def stream(self, endpoint, key, payload, cancelled):
                self.request = endpoint, key, payload
                return iter(["你好", "，", "我是 Muxiva。"])

            def cancel(self):
                self.cancelled = True

        client = Client()
        node = llm.QwenLlmStreamNode({}, lambda: client)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_process(TextFrame("你是谁？", sequence=301), Context("text_in"))
        self.assertTrue(wait_until(lambda: node._worker is not None and not node._worker.is_alive()))
        ctx = Context("tick_in")
        node.on_process(EventFrame("muxiva.runtime.tick"), ctx)
        self.assertEqual(
            [frame.text for port, frame in ctx.emissions if port == "text_out"],
            ["你好，我是 Muxiva。"],
        )
        self.assertEqual(ctx.emissions[0][1].sequence, 301)
        self.assertEqual(
            [frame.topic for port, frame in ctx.emissions if port == "event_out"],
            ["muxiva.voice.response.completed"],
        )
        self.assertFalse(any(port == "client_event_out" for port, _ in ctx.emissions))
        self.assertTrue(client.request[2]["stream"])
        self.assertEqual(ctx.events[0][0], "muxiva.voice.response.delta")
        self.assertEqual(ctx.events[-1][0], "muxiva.voice.response.completed")
        node.on_finish()

    def test_llm_signal_cancels_provider_and_discards_queued_old_output(self):
        started = threading.Event()

        class Client:
            def __init__(self):
                self.cancelled = threading.Event()

            def stream(self, _endpoint, _key, _payload, cancelled):
                started.set()
                yield "old sentence。"
                while not cancelled.wait(0.01):
                    pass

            def cancel(self):
                self.cancelled.set()

        client = Client()
        node = llm.QwenLlmStreamNode({}, lambda: client)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_process(TextFrame("old", sequence=100), Context("text_in"))
        self.assertTrue(started.wait(1))
        self.assertTrue(wait_until(lambda: not node._results.empty()))
        node.on_signal(SignalFrame("muxiva.voice.speech.started", sequence=200))
        self.assertTrue(client.cancelled.is_set())
        ctx = Context("tick_in")
        node.on_process(EventFrame("muxiva.runtime.tick"), ctx)
        self.assertEqual(ctx.emissions, [])
        node.on_finish()

    def test_tts_worker_drains_24k_pcm_and_preserves_sequence(self):
        transport = FakeTransport([
            {"type": "response.audio.delta", "delta": "AQIDBA=="},
            {"type": "response.done"},
        ])
        opened = []

        def factory(*_):
            opened.append(transport)
            return transport

        node = tts.QwenTtsRealtimeNode({}, factory)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()
        node.on_process(TextFrame("你好", sequence=404), Context("text_in"))
        node.on_process(TextFrame("世界", sequence=405), Context("text_in"))
        self.assertTrue(wait_until(lambda: len(transport.sent) == 4 and node._results.qsize() >= 4))
        ctx = Context("tick_in")
        node.on_process(EventFrame("muxiva.runtime.tick"), ctx)
        self.assertEqual(transport.sent[0]["type"], "input_text_buffer.append")
        self.assertEqual(transport.sent[1]["type"], "input_text_buffer.commit")
        self.assertEqual(ctx.emissions[0][1].sample_rate_hz, 24000)
        self.assertEqual(ctx.emissions[0][1].sequence, 404)
        self.assertEqual(ctx.emissions[0][1].data, b"\x01\x02\x03\x04")
        self.assertEqual(ctx.emissions[1][1].sequence, 405)
        self.assertEqual(len(opened), 1, "sentence chunks reuse one live TTS session")
        node.on_finish()

    def test_tts_signal_closes_active_session_and_clears_audio(self):
        active = threading.Event()

        class BlockingTransport(FakeTransport):
            def poll(self):
                active.set()
                yield {"type": "response.audio.delta", "delta": "AQIDBA=="}
                while not self.closed:
                    time.sleep(0.005)

        transport = BlockingTransport()
        node = tts.QwenTtsRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()
        node.on_process(TextFrame("old", sequence=10), Context("text_in"))
        self.assertTrue(active.wait(1))
        self.assertTrue(wait_until(lambda: not node._results.empty()))
        node.on_signal(SignalFrame("muxiva.voice.speech.started", sequence=20))
        self.assertTrue(transport.closed)
        ctx = Context("tick_in")
        node.on_process(EventFrame("muxiva.runtime.tick"), ctx)
        self.assertEqual(ctx.emissions, [])
        node.on_finish()

    def test_tts_rejects_stale_text_after_barge_in_without_a_separate_gate(self):
        transport = FakeTransport([{"type": "response.done"}])
        node = tts.QwenTtsRealtimeNode({}, lambda *_: transport)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()

        node.on_signal(SignalFrame("muxiva.voice.speech.started", sequence=20))
        stale = Context("text_in")
        node.on_process(TextFrame("旧回答", sequence=10), stale)
        self.assertEqual(stale.scheduled_ticks, [])
        self.assertTrue(node._jobs.empty())

        current = Context("text_in")
        node.on_process(TextFrame("新回答", sequence=21), current)
        self.assertEqual(current.scheduled_ticks, [20])
        self.assertTrue(wait_until(lambda: not node._jobs.empty() or not node._results.empty()))
        node.on_finish()

    def test_tts_connection_failure_is_reported_on_runtime_tick(self):
        def fail_to_connect(*_):
            raise OSError("connection refused")

        node = tts.QwenTtsRealtimeNode({}, fail_to_connect)
        with mock.patch.dict(os.environ, self.credentials):
            node.on_prepare()
        node.on_process(TextFrame("你好", sequence=500), Context("text_in"))
        self.assertTrue(wait_until(lambda: not node._results.empty()))
        with self.assertRaisesRegex(RuntimeError, "connection refused"):
            node.on_process(EventFrame("muxiva.runtime.tick"), Context("tick_in"))
        node.on_finish()

    def test_manifests_declare_cancellable_cascade_contracts(self):
        provider = json.loads((pathlib.Path(__file__).parents[2] / "muxiva.provider.json").read_text())
        self.assertEqual(provider["connections"][0]["id"], "dashscope")
        for package in ("qwen_realtime", "qwen_asr_realtime", "qwen_llm_stream", "qwen_tts_realtime"):
            manifest = json.loads((root / package / "muxiva.node.json").read_text())
            self.assertNotIn("provider_id", manifest)
            self.assertEqual(manifest["connection_id"], "dashscope")
            self.assertNotIn("connection", manifest)
            self.assertNotIn("client_event_out", {port["name"] for port in manifest["ports"]})
        asr_ports = {port["name"] for port in json.loads(
            (root / "qwen_asr_realtime" / "muxiva.node.json").read_text()
        )["ports"]}
        self.assertTrue(
            {"speech_out", "signal_out", "transcript_preview_out", "text_out", "event_out"}
            .issubset(asr_ports)
        )
        for package in ("qwen_llm_stream", "qwen_tts_realtime"):
            ports = {port["name"] for port in json.loads(
                (root / package / "muxiva.node.json").read_text()
            )["ports"]}
            self.assertIn("signal_in", ports)
        for package in ("qwen_llm_stream", "qwen_tts_realtime"):
            self.assertNotIn("tick_in", {port["name"] for port in json.loads(
                (root / package / "muxiva.node.json").read_text()
            )["ports"]})


if __name__ == "__main__":
    unittest.main()
