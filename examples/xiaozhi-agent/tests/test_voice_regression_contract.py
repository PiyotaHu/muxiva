"""Configuration guardrails distilled from real ESP32 voice regressions."""

from __future__ import annotations

import json
import pathlib
import unittest


PROJECT = pathlib.Path(__file__).resolve().parents[1]


class VoiceRegressionContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.graph = json.loads((PROJECT / "graph.json").read_text(encoding="utf-8"))
        cls.nodes = {node["id"]: node for node in cls.graph["nodes"]}
        cls.edges = {edge["id"]: edge for edge in cls.graph["edges"]}
        cls.pi_manifest = json.loads(
            (PROJECT.parent / "voice-agent" / ".muxiva" / "nodes" / "pi_agent" / "muxiva.node.json")
            .read_text(encoding="utf-8")
        )

    def test_playback_has_jitter_buffer_and_long_lossless_queue(self) -> None:
        config = self.nodes["xiaozhi-in"]["node_config"]
        self.assertGreaterEqual(config["playback_prebuffer_ms"], 1200)
        self.assertGreaterEqual(config["playback_queue_ms"], 120000)

    def test_final_asr_is_required_before_barge_in(self) -> None:
        config = self.nodes["qwen-vad-asr"]["node_config"]
        self.assertTrue(config["barge_in_requires_final"])
        self.assertTrue(config["ignore_filler_utterances"])

    def test_graph_is_acyclic_and_buildable(self) -> None:
        adjacency = {node_id: [] for node_id in self.nodes}
        indegree = {node_id: 0 for node_id in self.nodes}
        for edge in self.graph["edges"]:
            source = edge["from"]["node_id"]
            target = edge["to"]["node_id"]
            adjacency[source].append(target)
            indegree[target] += 1
        ready = [node_id for node_id, degree in indegree.items() if degree == 0]
        visited = 0
        while ready:
            source = ready.pop()
            visited += 1
            for target in adjacency[source]:
                indegree[target] -= 1
                if indegree[target] == 0:
                    ready.append(target)
        self.assertEqual(visited, len(self.nodes), "voice graph contains a directed cycle")

    def test_agent_completion_reaches_tts_drain_barrier(self) -> None:
        edge = self.edges["agent-state-to-tts"]
        self.assertEqual(edge["from"], {"node_id": "pi-agent", "port": "event_out"})
        self.assertEqual(edge["to"], {"node_id": "qwen-tts", "port": "event_in"})

    def test_agent_capability_packs_and_timeouts_are_explicit(self) -> None:
        config = self.nodes["pi-agent"]["node_config"]
        self.assertTrue(config["information_tools_enabled"])
        self.assertTrue(config["web_search_enabled"])
        self.assertTrue(config["device_tools_enabled"])
        self.assertTrue(config["artwork_tools_enabled"])
        self.assertFalse(config["workspace_tools_enabled"])
        self.assertGreater(config["agent_turn_timeout_ms"], config["web_search_timeout_ms"])
        self.assertGreaterEqual(config["max_tokens"], 768)

    def test_artwork_runtime_is_reproducibly_installable(self) -> None:
        requirements = (PROJECT / "requirements.txt").read_text(encoding="utf-8")
        setup = (PROJECT / "setup.sh").read_text(encoding="utf-8")
        self.assertIn("Pillow", requirements)
        self.assertIn("examples/xiaozhi-agent/requirements.txt", setup)
        self.assertTrue((PROJECT / ".muxiva" / "tools" / "prepare_image.py").is_file())
        self.assertTrue((PROJECT / ".muxiva" / "tools" / "build_gallery.py").is_file())

    def test_spoken_progress_is_disabled_in_the_realtime_graph(self) -> None:
        config = self.nodes["pi-agent"]["node_config"]
        self.assertEqual(config["progress_message"], "")
        self.assertEqual(config["progress_delay_ms"], 0)

    def test_transport_stop_policy_is_configurable(self) -> None:
        config = self.nodes["xiaozhi-in"]["node_config"]
        self.assertGreaterEqual(config["playback_stop_grace_ms"], 0)
        self.assertGreater(config["playback_no_audio_stop_timeout_ms"], 0)

    def test_pi_graph_configuration_is_declared_by_manifest(self) -> None:
        declared = set(self.pi_manifest["config_schema"]["properties"])
        configured = set(self.nodes["pi-agent"]["node_config"])
        self.assertEqual(configured - declared, set())


if __name__ == "__main__":
    unittest.main()
