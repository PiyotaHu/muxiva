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

    def test_voice_turn_controller_exclusively_owns_barge_in(self) -> None:
        asr = self.nodes["qwen-vad-asr"]["node_config"]
        policy = self.nodes["voice-turn"]["node_config"]
        self.assertNotIn("emit_legacy_barge_in_signal", asr)
        self.assertNotIn("ignore_filler_utterances", asr)
        self.assertNotIn("barge_in_requires_final", asr)
        self.assertTrue(policy["ignore_filler_utterances"])
        self.assertEqual(policy["early_cancel_preview_hits"], 1)
        self.assertIn("嗯", policy["ignored_utterances"])
        self.assertIn("额", policy["ignored_utterances"])
        self.assertIn("um", policy["ignored_utterances"])
        self.assertIn("eh", policy["ignored_utterances"])
        signal_sources = {
            edge["from"]["node_id"]
            for edge in self.graph["edges"]
            if edge["frame_type"] == "signal" and edge["to"]["node_id"] != "voice-turn"
        }
        self.assertEqual(signal_sources, {"voice-turn"})

    def test_raw_activity_is_observational_and_final_text_is_admitted_once(self) -> None:
        self.assertEqual(
            self.edges["speech-activity-to-turn-controller"]["to"],
            {"node_id": "voice-turn", "port": "activity_in"},
        )
        self.assertEqual(
            self.edges["asr-transcript-to-turn-controller"]["to"],
            {"node_id": "voice-turn", "port": "transcript_in"},
        )
        self.assertEqual(
            self.edges["turn-prompt-to-agent"]["from"],
            {"node_id": "voice-turn", "port": "prompt_out"},
        )
        self.assertEqual(
            self.edges["asr-preview-to-turn-controller"]["to"],
            {"node_id": "voice-turn", "port": "preview_in"},
        )

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

    def test_voice_presentation_is_downstream_of_the_generic_agent(self) -> None:
        formatter = self.nodes["speech-formatter"]["node_config"]
        self.assertEqual(
            self.edges["agent-to-formatter"]["from"],
            {"node_id": "pi-agent", "port": "text_out"},
        )
        self.assertEqual(
            self.edges["agent-to-events"]["from"],
            {"node_id": "speech-formatter", "port": "text_out"},
        )
        self.assertEqual(
            self.edges["agent-state-to-formatter"]["to"],
            {"node_id": "speech-formatter", "port": "event_in"},
        )
        self.assertIn("耳朵", formatter["suppressed_parenthetical_terms"])
        agent_config = self.nodes["pi-agent"]["node_config"]
        self.assertNotIn("strip_stage_directions", agent_config)
        self.assertNotIn("emotion_events_enabled", agent_config)
        self.assertNotIn("sentence_chunk_characters", agent_config)

    def test_tts_drain_barrier_reaches_the_final_audio_sink(self) -> None:
        edge = self.edges["tts-state-to-audio-sink"]
        self.assertEqual(edge["from"], {"node_id": "qwen-tts", "port": "event_out"})
        self.assertEqual(edge["to"], {"node_id": "xiaozhi-out", "port": "event_in"})

    def test_agent_capability_packs_and_timeouts_are_explicit(self) -> None:
        config = self.nodes["pi-agent"]["node_config"]
        self.assertTrue(config["information_tools_enabled"])
        self.assertTrue(config["web_search_enabled"])
        self.assertTrue(config["device_tools_enabled"])
        self.assertTrue(config["artwork_tools_enabled"])
        self.assertFalse(config["workspace_tools_enabled"])
        self.assertGreater(config["agent_request_timeout_ms"], config["web_search_timeout_ms"])
        self.assertEqual(config["web_search_evidence_max_tokens"], 256)
        self.assertTrue(config["web_search_streaming"])
        self.assertGreaterEqual(config["max_tokens"], 768)

    def test_product_persona_keeps_cat_flavor_subtle(self) -> None:
        prompt = self.nodes["pi-agent"]["node_config"]["system_prompt"]
        self.assertIn("首要任务是准确、高效地帮助用户", prompt)
        self.assertIn("而不是表演猫咪", prompt)
        self.assertIn("每次回复最多一次", prompt)
        self.assertIn("严肃问题", prompt)
        self.assertIn("正常助手语气", prompt)
        self.assertIn("不要主动使用", prompt)
        for cliche in ("小鱼干", "毛线球", "猫爪"):
            self.assertIn(cliche, prompt)

    def test_artwork_runtime_is_reproducibly_installable(self) -> None:
        requirements = (PROJECT / "requirements.txt").read_text(encoding="utf-8")
        setup = (PROJECT / "setup.sh").read_text(encoding="utf-8")
        self.assertIn("Pillow", requirements)
        self.assertIn("examples/xiaozhi-agent/requirements.txt", setup)
        self.assertTrue((PROJECT / ".muxiva" / "tools" / "prepare_image.py").is_file())
        self.assertTrue((PROJECT / ".muxiva" / "tools" / "build_gallery.py").is_file())

    def test_artwork_prompt_targets_the_monochrome_display(self) -> None:
        prompt = self.nodes["pi-agent"]["node_config"]["artwork_style_prompt"]
        self.assertIn("400×300", prompt)
        self.assertIn("黑白线稿", prompt)
        self.assertIn("高对比", prompt)
        self.assertIn("不要使用彩色", prompt)

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
