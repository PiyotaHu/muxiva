import importlib.util
import json
import pathlib
import sys
import types
import unittest


class ByteFrame:
    def __init__(self, data, media_type="application/octet-stream", sequence=0):
        self.data = bytes(data)
        self.media_type = media_type
        self.sequence = sequence


shim = types.ModuleType("muxiva")
shim.ByteFrame = ByteFrame
sys.modules["muxiva"] = shim

source = (
    pathlib.Path(__file__).parents[1]
    / ".muxiva/nodes/voice_room_event_encoder/node.py"
)
spec = importlib.util.spec_from_file_location("voice_room_event_encoder", source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class EventFrame:
    topic = "muxiva.voice.response.delta"
    payload = '{"text":"hello"}'
    source = "qwen.audio_realtime"
    stream_id = "stream-1"
    trace_id = "trace-1"
    sequence = 42
    timestamp_ns = 100


class Context:
    def __init__(self, input_port="event_in"):
        self.input_port = input_port
        self.emissions = []

    def emit(self, port, frame):
        self.emissions.append((port, frame))


class VoiceRoomProtocolTests(unittest.TestCase):
    def test_event_is_encoded_without_transport_fragmentation(self):
        node = module.VoiceRoomEventEncoderNode()
        context = Context()
        node.on_process(EventFrame(), context)

        self.assertEqual(len(context.emissions), 1)
        port, frame = context.emissions[0]
        self.assertEqual(port, "message_out")
        self.assertEqual(frame.media_type, module.MEDIA_TYPE)
        envelope = json.loads(frame.data)
        self.assertEqual(envelope["version"], "muxiva.client-event/v1")
        self.assertEqual(envelope["type"], EventFrame.topic)
        self.assertEqual(envelope["payload"], {"text": "hello"})
        self.assertNotIn("fragment_count", envelope)

    def test_signal_cancels_stale_response_messages(self):
        node = module.VoiceRoomEventEncoderNode()
        node.on_signal(types.SimpleNamespace(sequence=42))
        context = Context()
        node.on_process(EventFrame(), context)
        self.assertEqual(context.emissions, [])

    def test_semantic_text_ports_become_application_events(self):
        node = module.VoiceRoomEventEncoderNode()
        cases = {
            "transcript_preview_in": "muxiva.voice.transcript.preview",
            "transcript_in": "muxiva.voice.transcript.completed",
            "response_text_in": "muxiva.voice.response.delta",
        }
        for input_port, expected_topic in cases.items():
            with self.subTest(input_port=input_port):
                context = Context(input_port)
                frame = types.SimpleNamespace(
                    text="hello",
                    sequence=43,
                    stream_id="stream-1",
                    trace_id="trace-1",
                    timestamp_ns=100,
                )
                node.on_process(frame, context)
                envelope = json.loads(context.emissions[0][1].data)
                self.assertEqual(envelope["type"], expected_topic)
                self.assertEqual(envelope["payload"], {"text": "hello"})

    def test_generic_agent_lifecycle_is_mapped_at_the_application_boundary(self):
        node = module.VoiceRoomEventEncoderNode()
        context = Context()
        frame = types.SimpleNamespace(
            topic="muxiva.agent.response.completed",
            payload={"text": "done"},
            source="pi.agent",
            stream_id="stream-1",
            trace_id="trace-1",
            sequence=44,
            timestamp_ns=100,
        )
        node.on_process(frame, context)
        envelope = json.loads(context.emissions[0][1].data)
        self.assertEqual(envelope["type"], "muxiva.voice.response.completed")
        self.assertEqual(envelope["payload"], {"text": "done"})

    def test_cascade_graph_exposes_only_domain_nodes_not_runtime_plumbing(self):
        project = pathlib.Path(__file__).parents[1]
        graphs = [
            json.loads((project / "graph.json").read_text()),
            json.loads((project / ".muxiva/templates/02-qwen-cascade.json").read_text())["graph"],
        ]
        for graph in graphs:
            with self.subTest(graph=graph["graph_id"]):
                node_types = {node["node_type"] for node in graph["nodes"]}
                self.assertTrue({
                    "builtin.interval_tick",
                    "builtin.voice_turn_context",
                    "builtin.text_cancellation_gate",
                }.isdisjoint(node_types))
                self.assertFalse(any(
                    edge["to"]["port"] == "tick_in" for edge in graph["edges"]
                ))
                routes = {
                    (edge["from"]["node_id"], edge["to"]["node_id"])
                    for edge in graph["edges"]
                }
                self.assertIn(("qwen-vad-asr", "pi-agent"), routes)
                self.assertIn(("pi-agent", "qwen-tts"), routes)


if __name__ == "__main__":
    unittest.main()
