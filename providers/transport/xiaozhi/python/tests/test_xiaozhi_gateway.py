"""End-to-end Xiaozhi transport tests.

These tests simulate a real Xiaozhi ESP32 device: they connect a WebSocket
client, send a ``hello``, stream Opus microphone audio, send ``abort``, and
verify that the gateway both decodes ingress PCM and streams Opus + ``stt`` /
``tts`` JSON back to the device.

Run from the repository root:

    python3 -m unittest discover -s providers/transport/xiaozhi/python/tests -v

Requirements: ``libopus0`` (system) and the ``websockets`` Python package.
"""

from __future__ import annotations

import asyncio
import array
import json
import math
import pathlib
import socket
import sys
import time
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).parents[1]))

try:
    import opus_codec
    import xiaozhi_gateway
except Exception:  # libopus missing, for example
    opus_codec = None
    xiaozhi_gateway = None

try:
    import websockets
except ImportError:
    websockets = None

SAMPLE_RATE = 16_000
FRAME_DURATION_MS = 60
FRAME_SIZE = SAMPLE_RATE * FRAME_DURATION_MS // 1000


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def sine_pcm(
    frequency: float = 440.0,
    amplitude: int = 12_000,
    duration_ms: int = FRAME_DURATION_MS,
    sample_rate: int = SAMPLE_RATE,
) -> bytes:
    """One frame of 16-bit little-endian mono sine-wave PCM."""
    samples = []
    for index in range(sample_rate * duration_ms // 1000):
        value = int(amplitude * math.sin(2.0 * math.pi * frequency * index / sample_rate))
        samples.append(value)
    return b"".join(value.to_bytes(2, "little", signed=True) for value in samples)


def has_energy(pcm: bytes, threshold: int = 100) -> bool:
    samples = array.array("h")
    samples.frombytes(pcm)
    return any(abs(value) > threshold for value in samples)


@unittest.skipUnless(
    xiaozhi_gateway is not None and websockets is not None,
    "xiaozhi gateway tests require libopus and the websockets package",
)
class XiaozhiGatewayTests(unittest.TestCase):
    def setUp(self) -> None:
        self.ws_port = free_port()
        self.control_port = free_port()
        self.gateway = xiaozhi_gateway.XiaozhiGateway(
            {
                "ws_host": "127.0.0.1",
                "ws_port": self.ws_port,
                "control_host": "127.0.0.1",
                "control_port": self.control_port,
                "sample_rate": SAMPLE_RATE,
                "frame_duration_ms": FRAME_DURATION_MS,
                "playback_initial_burst_interval_ms": 12,
            }
        )
        self.gateway.start()
        # Give the WebSocket and control servers a moment to bind.
        time.sleep(0.3)

    def tearDown(self) -> None:
        self.gateway.stop()

    def test_device_session_roundtrip(self) -> None:
        asyncio.run(self._run_device_session())

    def test_latest_connection_owns_device_session(self) -> None:
        asyncio.run(self._run_connection_takeover())

    async def _run_connection_takeover(self) -> None:
        uri = f"ws://127.0.0.1:{self.ws_port}"
        async with websockets.connect(uri) as first:
            await first.send(json.dumps({"type": "hello"}))
            first_hello = json.loads(
                await asyncio.wait_for(first.recv(), timeout=5)
            )

            async with websockets.connect(uri) as second:
                await second.send(json.dumps({"type": "hello"}))
                second_hello = json.loads(
                    await asyncio.wait_for(second.recv(), timeout=5)
                )
                self.assertNotEqual(
                    first_hello["session_id"], second_hello["session_id"]
                )

                # The previous handler's delayed cleanup used to clear _ws
                # after the replacement was already live. Wait until that old
                # handler has fully unwound, then prove the new session still
                # owns both request and response traffic.
                await asyncio.wait_for(first.wait_closed(), timeout=3)
                self.assertTrue(self.gateway.has_client())
                self.assertEqual(
                    self.gateway._client_id, second_hello["session_id"]
                )

                await second.send(json.dumps({"type": "ping"}))
                pong = json.loads(await asyncio.wait_for(second.recv(), timeout=2))
                self.assertEqual(pong["type"], "pong")

                self.assertTrue(
                    self.gateway.publish_message(
                        {"type": "stt", "text": "replacement-live"}
                    )
                )
                message = json.loads(
                    await asyncio.wait_for(second.recv(), timeout=2)
                )
                self.assertEqual(message["text"], "replacement-live")
                self.assertEqual(
                    message["session_id"], second_hello["session_id"]
                )

    async def _run_device_session(self) -> None:
        encoder = opus_codec.OpusEncoder(
            sample_rate=SAMPLE_RATE, frame_duration_ms=FRAME_DURATION_MS
        )
        decoder = opus_codec.OpusDecoder(
            sample_rate=SAMPLE_RATE, frame_duration_ms=FRAME_DURATION_MS
        )
        microphone_pcm = sine_pcm()
        microphone_packet = encoder.encode(microphone_pcm)

        async with websockets.connect(f"ws://127.0.0.1:{self.ws_port}") as device:
            # 1. Handshake: server replies with negotiated Opus audio params.
            await device.send(json.dumps({"type": "hello"}))
            hello = json.loads(await asyncio.wait_for(device.recv(), timeout=5))
            self.assertEqual(hello["type"], "hello")
            self.assertEqual(hello["audio_params"]["format"], "opus")
            self.assertEqual(hello["audio_params"]["sample_rate"], SAMPLE_RATE)
            self.assertEqual(hello["audio_params"]["frame_duration"], FRAME_DURATION_MS)
            self.assertTrue(hello["session_id"])

            # 2. Stream a short simulated microphone utterance.
            for _ in range(3):
                await device.send(microphone_packet)
            await asyncio.sleep(0.2)
            ingress = self.gateway.poll_audio()
            self.assertGreaterEqual(len(ingress), 3)
            self.assertEqual(len(ingress[0]), FRAME_SIZE * 2)
            self.assertTrue(has_energy(ingress[0]), "decoded ingress audio must not be silent")

            # 3. Device interrupt (button / barge-in).
            await device.send(json.dumps({"type": "abort"}))
            await asyncio.sleep(0.2)
            events = self.gateway.poll_events()
            self.assertTrue(any(event.get("type") == "abort" for event in events))

            # 4. Assistant egress audio: publish PCM through the sink control client.
            sink = xiaozhi_gateway.XiaozhiControlClient(
                "127.0.0.1", self.control_port, "sink"
            )
            self.assertTrue(sink.connect())
            # Send 2.5 protocol frames in one vendor-style delta. The gateway
            # must preserve all samples, not truncate the chunk to one frame.
            sink.send(
                {
                    "op": "audio",
                    "pcm_hex": (microphone_pcm * 2 + microphone_pcm[: len(microphone_pcm) // 2]).hex(),
                }
            )

            # Regression: assistant playback must never mute microphone input.
            # Echo rejection belongs after ASR validation so a real user can
            # still say "stop" or ask a new question while audio is playing.
            await asyncio.sleep(0.1)
            await device.send(microphone_packet)
            await asyncio.sleep(0.1)
            live_ingress = self.gateway.poll_audio()
            self.assertTrue(
                live_ingress,
                "microphone ingress was muted during assistant playback",
            )

            # 5. Device display messages through the event-encoder control client.
            events = xiaozhi_gateway.XiaozhiControlClient(
                "127.0.0.1", self.control_port, "events"
            )
            self.assertTrue(events.connect())
            events.send({"op": "message", "payload": {"type": "stt", "text": "你好小智"}})
            events.send(
                {
                    "op": "message",
                    "payload": {"type": "tts", "state": "sentence_start", "text": "你好呀"},
                }
            )
            # The audio sink owns the final media barrier, so its control
            # connection must be allowed to enqueue the deferred stop after
            # every PCM frame has crossed the graph.
            sink.send(
                {"op": "message", "payload": {"type": "tts", "state": "stop"}}
            )

            # 6. The device must receive Opus audio plus stt/tts JSON messages.
            received_audio = False
            received_audio_packets = 0
            received_stt = False
            received_tts = False
            received_stop = False
            audio_received_at = 0.0
            audio_packet_times = []
            deadline = time.time() + 5.0
            while time.time() < deadline and not received_stop:
                try:
                    message = await asyncio.wait_for(device.recv(), timeout=1.0)
                except asyncio.TimeoutError:
                    continue
                if isinstance(message, (bytes, bytearray)):
                    egress_pcm = decoder.decode(bytes(message))
                    self.assertEqual(len(egress_pcm), FRAME_SIZE * 2)
                    self.assertTrue(has_energy(egress_pcm), "egress audio must not be silent")
                    received_audio = True
                    received_audio_packets += 1
                    audio_received_at = time.monotonic()
                    audio_packet_times.append(audio_received_at)
                else:
                    payload = json.loads(message)
                    if payload.get("type") == "stt":
                        received_stt = True
                        self.assertEqual(payload["text"], "你好小智")
                    elif payload.get("type") == "tts":
                        if payload["state"] == "sentence_start":
                            received_tts = True
                        elif payload["state"] == "stop":
                            received_stop = True
                            self.assertTrue(received_audio, "tts stop arrived before audio")
                            self.assertGreaterEqual(
                                time.monotonic() - audio_received_at,
                                0.8,
                                "tts stop was not held until audio became quiet",
                            )
                    # The gateway injects the session id into display messages.
                    self.assertTrue(payload.get("session_id"))

            self.assertTrue(received_audio, "device never received Opus audio")
            self.assertEqual(received_audio_packets, 3, "PCM delta was not reblocked losslessly")
            startup_gap = audio_packet_times[1] - audio_packet_times[0]
            self.assertGreaterEqual(startup_gap, 0.006)
            self.assertLess(startup_gap, 0.05)
            self.assertGreaterEqual(
                audio_packet_times[-1] - audio_packet_times[0],
                0.8,
                "the padded final half-frame was not held until TTS became quiet",
            )
            self.assertTrue(received_stt, "device never received an stt message")
            self.assertTrue(received_tts, "device never received a tts message")
            self.assertTrue(received_stop, "device never received the deferred tts stop")

            sink.close()
            events.close()

    def test_startup_window_is_spaced_then_audio_is_realtime_paced(self) -> None:
        asyncio.run(self._run_spaced_startup_window())

    async def _run_spaced_startup_window(self) -> None:
        encoder = opus_codec.OpusEncoder(
            sample_rate=SAMPLE_RATE, frame_duration_ms=FRAME_DURATION_MS
        )
        microphone_pcm = sine_pcm()
        async with websockets.connect(f"ws://127.0.0.1:{self.ws_port}") as device:
            await device.send(json.dumps({"type": "hello"}))
            await asyncio.wait_for(device.recv(), timeout=5)
            sink = xiaozhi_gateway.XiaozhiControlClient(
                "127.0.0.1", self.control_port, "sink"
            )
            self.assertTrue(sink.connect())
            # Default startup threshold is 20 frames. Publish enough audio to
            # start playback and leave packets for the paced phase.
            sink.send({"op": "audio", "pcm_hex": (microphone_pcm * 24).hex()})
            packet_times = []
            deadline = time.monotonic() + 5
            while len(packet_times) < 8 and time.monotonic() < deadline:
                message = await asyncio.wait_for(device.recv(), timeout=2)
                if isinstance(message, (bytes, bytearray)):
                    packet_times.append(time.monotonic())
            self.assertEqual(len(packet_times), 8)
            startup_intervals = [
                packet_times[index] - packet_times[index - 1]
                for index in range(1, 5)
            ]
            self.assertTrue(
                all(0.006 <= interval <= 0.05 for interval in startup_intervals),
                f"startup packets were not safely spaced: {startup_intervals}",
            )
            paced_intervals = [
                packet_times[index] - packet_times[index - 1]
                for index in range(5, len(packet_times))
            ]
            self.assertTrue(
                all(0.04 <= interval <= 0.12 for interval in paced_intervals),
                f"post-startup audio was not real-time paced: {paced_intervals}",
            )
            sink.close()
            encoder.close()


if __name__ == "__main__":
    unittest.main()
