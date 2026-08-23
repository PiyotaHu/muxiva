import importlib.util
import pathlib
import sys
import types
import unittest


class FakeControlClient:
    def __init__(self, **_):
        self.commands = []
        self.connected = True

    def connect(self):
        self.connected = True
        return True

    def is_connected(self):
        return self.connected

    def send(self, command):
        self.commands.append(command)

    def close(self):
        self.connected = False


gateway = types.ModuleType("xiaozhi_gateway")
gateway.STT_TYPE = "stt"
gateway.TTS_TYPE = "tts"
gateway.XiaozhiControlClient = FakeControlClient
previous_gateway = sys.modules.get("xiaozhi_gateway")
try:
    sys.modules["xiaozhi_gateway"] = gateway
    node_path = (
        pathlib.Path(__file__).parents[1]
        / "nodes"
        / "xiaozhi_event_encoder"
        / "node.py"
    )
    spec = importlib.util.spec_from_file_location("xiaozhi_event_encoder_test", node_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
finally:
    if previous_gateway is None:
        del sys.modules["xiaozhi_gateway"]
    else:
        sys.modules["xiaozhi_gateway"] = previous_gateway


class Frame:
    def __init__(self, topic="", text="", sequence=1, payload=""):
        self.name = "muxiva.turn.cancelled"
        self.topic = topic
        self.text = text
        self.sequence = sequence
        self.payload = payload


class Context:
    def __init__(self, input_port):
        self.input_port = input_port


class XiaozhiEventEncoderTests(unittest.TestCase):
    def test_normal_tts_drain_is_owned_by_audio_sink(self):
        node = module.XiaozhiEventEncoderNode()
        node.on_process(Frame(text="问题", sequence=10), Context("transcript_in"))
        commands_before_drain = len(node._client.commands)

        node.on_process(
            Frame(topic="muxiva.voice.tts.drained", sequence=10),
            Context("event_in"),
        )
        self.assertEqual(len(node._client.commands), commands_before_drain)
        self.assertFalse(node._speaking)

        node = module.XiaozhiEventEncoderNode()
        node.on_process(Frame(text="问题", sequence=10), Context("transcript_in"))
        commands_before_completion = len(node._client.commands)

        node.on_process(
            Frame(topic="muxiva.agent.response.completed", sequence=10),
            Context("event_in"),
        )
        self.assertEqual(len(node._client.commands), commands_before_completion)

    def test_failed_turn_also_requests_stop(self):
        node = module.XiaozhiEventEncoderNode()
        node.on_process(
            Frame(topic="muxiva.agent.response.failed", sequence=11),
            Context("event_in"),
        )
        self.assertEqual(node._client.commands[-1]["payload"]["state"], "stop")

    def test_emotion_event_is_mapped_without_inspecting_spoken_text(self):
        node = module.XiaozhiEventEncoderNode()
        node.on_process(
            Frame(topic="muxiva.agent.emotion.changed", payload='{"emotion":"happy"}'),
            Context("event_in"),
        )
        self.assertEqual(
            node._client.commands[-1]["payload"],
            {"type": "llm", "emotion": "happy"},
        )

        before = len(node._client.commands)
        node.on_process(
            Frame(text="哈哈，这句话本身不应由传输层做情绪推断。"),
            Context("response_text_in"),
        )
        self.assertEqual(len(node._client.commands), before + 1)
        self.assertEqual(node._client.commands[-1]["payload"]["type"], "tts")

    def test_invalid_emotion_event_is_ignored(self):
        node = module.XiaozhiEventEncoderNode()
        before = len(node._client.commands)
        node.on_process(
            Frame(topic="muxiva.agent.emotion.changed", payload='{"emotion":"unknown"}'),
            Context("event_in"),
        )
        self.assertEqual(len(node._client.commands), before)

    def test_raw_vad_activity_never_clears_active_playback(self):
        node = module.XiaozhiEventEncoderNode()
        node.on_process(Frame(text="正在播报的问题", sequence=20), Context("transcript_in"))
        before = list(node._client.commands)

        node.on_process(
            Frame(topic="muxiva.voice.speech.started", sequence=21),
            Context("event_in"),
        )
        node.on_process(
            Frame(topic="muxiva.voice.speech.stopped", sequence=22),
            Context("event_in"),
        )

        self.assertEqual(node._client.commands, before)
        self.assertTrue(node._speaking)

    def test_validated_barge_in_signal_still_clears_playback(self):
        node = module.XiaozhiEventEncoderNode()
        node.on_process(Frame(text="正在播报的问题", sequence=20), Context("transcript_in"))

        node.on_signal(Frame(sequence=30))

        self.assertIn({"op": "reset"}, node._client.commands)
        self.assertEqual(node._client.commands[-1]["payload"]["state"], "stop")
        self.assertFalse(node._speaking)


if __name__ == "__main__":
    unittest.main()
