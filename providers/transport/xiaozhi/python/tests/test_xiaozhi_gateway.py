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
            }
        )
        self.gateway.start()
        # Give the WebSocket and control servers a moment to bind.
        time.sleep(0.3)

    def tearDown(self) -> None:
        self.gateway.stop()

    def test_device_session_roundtrip(self) -> None:
        asyncio.run(self._run_device_session())

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
            # The LLM may finish before asynchronous TTS audio reaches the
            # gateway.  A stop marker must therefore remain behind the audio.
            events.send(
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
            self.assertGreaterEqual(
                audio_packet_times[-1] - audio_packet_times[0],
                0.09,
                "Opus packets were burst-sent instead of paced near real time",
            )
            self.assertTrue(received_stt, "device never received an stt message")
            self.assertTrue(received_tts, "device never received a tts message")
            self.assertTrue(received_stop, "device never received the deferred tts stop")

            sink.close()
            events.close()


if __name__ == "__main__":
    unittest.main()
