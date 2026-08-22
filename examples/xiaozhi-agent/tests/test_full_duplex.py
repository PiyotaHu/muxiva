"""Reproducible full-duplex Xiaozhi <-> Muxiva interaction test.

This test simulates a real Xiaozhi ESP32 device and drives a three-turn
conversation through the complete Muxiva pipeline (VAD + ASR + LLM + TTS):

    turn 1  user:  "你好吗？"          server: greeting
    turn 2  user:  "给我说一个笑话"     server: starts telling a joke
    turn 3  user interrupts with "今天天气怎么样"
                                      server: barge-in, answers weather

The user voice is synthesized by Qwen TTS (``make_fixtures.py``) and streamed
into the server as 16 kHz Opus, exactly like the device microphone. The server
is started as a subprocess, so the whole case is reproducible end to end.

Requirements
------------
* Build the CLI once: ``cargo build -p muxiva-cli``.
* Generate fixtures: ``make_fixtures.py``.
* Credentials in the environment (or ``examples/xiaozhi-agent/.env``):
  - ``DASHSCOPE_API_KEY``        (Qwen ASR / TTS / LLM)
  - ``DASHSCOPE_WORKSPACE_ID``   (Qwen realtime ASR endpoint)

Run
---
    cd examples/xiaozhi-agent
    DASHSCOPE_API_KEY=... DASHSCOPE_WORKSPACE_ID=... \
        python3 -m unittest tests.test_full_duplex -v

The credentials are never written to disk by this test.
"""

from __future__ import annotations

import asyncio
import http.client
import json
import os
import pathlib
import signal
import socket
import subprocess
import sys
import time
import unittest
import wave

HERE = pathlib.Path(__file__).resolve().parent
PROJECT = HERE.parent
REPO_ROOT = PROJECT.parent.parent
FIXTURES = HERE / "fixtures"
SERVER_LOG = HERE / "server.log"
WS_HOST = "127.0.0.1"
WS_PORT = 8888
CLIENT_API_PORT = 18090
USE_EXISTING_SERVER = os.environ.get("MUXIVA_USE_EXISTING_SERVER", "").lower() in {
    "1", "true", "yes",
}
if USE_EXISTING_SERVER:
    CLIENT_API_PORT = int(os.environ.get("MUXIVA_EXISTING_API_PORT", "8080"))

sys.path.insert(0, str(REPO_ROOT / "providers" / "transport" / "xiaozhi" / "python"))
try:
    import opus_codec
except Exception:
    opus_codec = None

try:
    import websockets
except ImportError:
    websockets = None


def env_value(name: str) -> str:
    value = os.environ.get(name, "").strip()
    if value:
        return value
    env_file = PROJECT / ".env"
    if env_file.is_file():
        for line in env_file.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith(f"{name}="):
                value = line.split("=", 1)[1].strip().strip("'\"")
                if value:
                    return value
    return ""


def read_wav_pcm(path: pathlib.Path) -> bytes:
    with wave.open(str(path), "rb") as wav:
        assert wav.getnchannels() == 1, "fixtures must be mono"
        assert wav.getsampwidth() == 2, "fixtures must be 16-bit"
        assert wav.getframerate() == 16_000, "fixtures must be 16 kHz"
        return wav.readframes(wav.getnframes())


def wait_for_port(host: str, port: int, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.5):
                return True
        except OSError:
            time.sleep(0.2)
    return False


def wait_for_health(port: int, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
            connection.request("GET", "/healthz")
            response = connection.getresponse()
            response.read()
            connection.close()
            if response.status == 200:
                return True
        except OSError:
            pass
        time.sleep(0.2)
    return False


class DeviceClient:
    """Async stand-in for the Xiaozhi ESP32 firmware."""

    def __init__(self) -> None:
        self.ws = None
        self.stt_texts: list[str] = []
        self.tts_texts: list[str] = []
        self.tts_states: list[tuple[str, str]] = []
        self.audio_packets = 0
        self._receiver = None

    async def connect(self) -> None:
        self.ws = await websockets.connect(f"ws://{WS_HOST}:{WS_PORT}")
        await self.ws.send(json.dumps({"type": "hello"}))
        hello = json.loads(await asyncio.wait_for(self.ws.recv(), timeout=5))
        assert hello["type"] == "hello", hello
        self._receiver = asyncio.create_task(self._receive_loop())

    async def _receive_loop(self) -> None:
        try:
            async for message in self.ws:
                if isinstance(message, (bytes, bytearray)):
                    self.audio_packets += 1
                    continue
                payload = json.loads(message)
                kind = payload.get("type")
                if kind == "stt":
                    self.stt_texts.append(payload.get("text", ""))
                elif kind == "tts":
                    state = payload.get("state", "")
                    text = payload.get("text") or ""
                    self.tts_states.append((state, text))
                    if text:
                        self.tts_texts.append(text)
        except Exception:
            pass

    async def stream_wav(
        self,
        path: pathlib.Path,
        frame_ms: int = 60,
        trailing_silence_ms: int = 1500,
    ) -> None:
        # The real device streams continuously (including silence), so the
        # server VAD needs a trailing silence tail to close the utterance.
        encoder = opus_codec.OpusEncoder(sample_rate=16_000, frame_duration_ms=frame_ms)
        pcm = read_wav_pcm(path)
        frame_bytes = 16_000 * frame_ms // 1000 * 2
        frames = [
            (pcm[offset : offset + frame_bytes] + b"\x00" * frame_bytes)[:frame_bytes]
            for offset in range(0, len(pcm), frame_bytes)
        ]
        silence = b"\x00" * frame_bytes
        frames.extend([silence] * (trailing_silence_ms // frame_ms))
        for chunk in frames:
            await self.ws.send(encoder.encode(chunk))
            await asyncio.sleep(frame_ms / 1000.0)
        encoder.close()

    async def close(self) -> None:
        if self._receiver is not None:
            self._receiver.cancel()
        if self.ws is not None:
            await self.ws.close()


def join_text(texts: list[str]) -> str:
    return " ".join(texts)


async def wait_for(predicate, timeout: float, description: str) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if predicate():
            return
        await asyncio.sleep(0.1)
    raise AssertionError(f"timed out waiting for {description}")


@unittest.skipUnless(
    websockets is not None and opus_codec is not None,
    "full-duplex test requires libopus and the websockets package",
)
class FullDuplexTests(unittest.IsolatedAsyncioTestCase):
    server_process = None

    @classmethod
    def setUpClass(cls) -> None:
        if (
            not USE_EXISTING_SERVER
            and (not env_value("DASHSCOPE_API_KEY") or not env_value("DASHSCOPE_WORKSPACE_ID"))
        ):
            raise unittest.SkipTest(
                "DASHSCOPE_API_KEY and DASHSCOPE_WORKSPACE_ID are required"
            )
        missing = [
            path.name for path in (
                FIXTURES / "user_01_hello.wav",
                FIXTURES / "user_02_joke.wav",
                FIXTURES / "user_03_weather.wav",
            ) if not path.is_file()
        ]
        if missing:
            raise unittest.SkipTest(
                f"missing fixtures {missing}; run tests/make_fixtures.py first"
            )
        if USE_EXISTING_SERVER:
            if not wait_for_health(CLIENT_API_PORT, 20):
                raise RuntimeError("existing Muxiva service is not healthy")
            if not wait_for_port(WS_HOST, WS_PORT, 20):
                raise RuntimeError("existing Xiaozhi WebSocket port is not open")
        else:
            cls._start_server()

    @classmethod
    def tearDownClass(cls) -> None:
        if not USE_EXISTING_SERVER:
            cls._stop_server()

    @classmethod
    def _start_server(cls) -> None:
        binary = REPO_ROOT / "target" / "debug" / "muxiva"
        command = (
            [str(binary), "serve", "graph.json", "--host", "127.0.0.1", "--port", str(CLIENT_API_PORT)]
            if binary.is_file()
            else ["cargo", "run", "-p", "muxiva-cli", "--", "serve", "graph.json",
                  "--host", "127.0.0.1", "--port", str(CLIENT_API_PORT)]
        )
        env = os.environ.copy()
        env["DASHSCOPE_API_KEY"] = env_value("DASHSCOPE_API_KEY")
        env["DASHSCOPE_WORKSPACE_ID"] = env_value("DASHSCOPE_WORKSPACE_ID")
        cls.log_file = open(SERVER_LOG, "wb")
        cls.server_process = subprocess.Popen(
            command,
            cwd=PROJECT,
            env=env,
            stdout=cls.log_file,
            stderr=subprocess.STDOUT,
        )
        if not wait_for_health(CLIENT_API_PORT, 60):
            cls._dump_server_log()
            cls._stop_server()
            raise RuntimeError("Muxiva server did not become ready")
        if not wait_for_port(WS_HOST, WS_PORT, 20):
            cls._dump_server_log()
            cls._stop_server()
            raise RuntimeError("Xiaozhi WebSocket port never opened")

    @classmethod
    def _dump_server_log(cls) -> None:
        if SERVER_LOG.is_file():
            tail = SERVER_LOG.read_text(encoding="utf-8", errors="replace").splitlines()[-25:]
            print("\n[server.log tail]", "\n".join(tail), sep="\n")

    @classmethod
    def _stop_server(cls) -> None:
        process = cls.server_process
        cls.server_process = None
        if process is not None:
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
        log_file = getattr(cls, "log_file", None)
        cls.log_file = None
        if log_file is not None:
            log_file.close()

    async def asyncSetUp(self) -> None:
        self.device = DeviceClient()
        await self.device.connect()

    async def asyncTearDown(self) -> None:
        await self.device.close()

    async def test_three_turn_duplex_with_barge_in(self) -> None:
        device = self.device
        turn_log = []

        def snapshot() -> tuple[str, str]:
            return (" ".join(device.stt_texts), " ".join(device.tts_texts))

        def states_named(state: str) -> list[str]:
            return [text for (name, text) in device.tts_states if name == state]

        # Turn 1: greeting.
        await device.stream_wav(FIXTURES / "user_01_hello.wav")
        await wait_for(
            lambda: any("你好" in text for text in device.stt_texts),
            15, "the greeting transcript",
        )
        await wait_for(lambda: device.tts_texts, 20, "the greeting answer")
        await wait_for(lambda: states_named("start"), 20, "the speaking state")
        await wait_for(lambda: device.audio_packets > 0, 20, "assistant audio")
        turn_log.append(("1 greeting", *snapshot()))

        # Turn 2: ask for a joke.
        device.stt_texts.clear()
        device.tts_texts.clear()
        await device.stream_wav(FIXTURES / "user_02_joke.wav")
        await wait_for(
            lambda: any("笑话" in text for text in device.stt_texts),
            15, "the joke transcript",
        )
        await wait_for(lambda: device.tts_texts, 20, "the joke answer")
        turn_log.append(("2 joke", *snapshot()))

        # Turn 3: barge in while the assistant is still speaking.
        # Give the joke TTS a moment to start, then overlap it with the weather
        # question; the server VAD must interrupt and switch to the new turn.
        await asyncio.sleep(0.5)
        device.stt_texts.clear()
        device.tts_texts.clear()
        await device.stream_wav(FIXTURES / "user_03_weather.wav")
        await wait_for(
            lambda: any("天气" in text for text in device.stt_texts),
            15, "the weather transcript",
        )
        # The LLM wording is non-deterministic, so only require a fresh spoken
        # response after the barge-in rather than specific weather keywords.
        await wait_for(lambda: device.tts_texts, 20, "a response after the barge-in")
        # Audio is deliberately paced in real time.  The response text reaches
        # the display before the last Opus packet is played, so wait for the
        # final stop marker instead of asserting immediately after the text.
        await wait_for(
            lambda: states_named("stop"),
            60,
            "the final paced response to finish playing",
        )
        turn_log.append(("3 weather (barge-in)", *snapshot()))

        self.assertTrue(device.tts_texts, "expected a spoken response after barge-in")

        # Display protocol: each turn must show an ASR question (stt), enter the
        # speaking state (tts start), stream the answer (tts sentence_start), and
        # leave the speaking state (tts stop).
        self.assertGreaterEqual(
            len(states_named("start")), 3, "every turn should enter the speaking state"
        )
        self.assertGreaterEqual(
            len(states_named("sentence_start")),
            3,
            "every turn should display an answer sentence",
        )
        self.assertGreaterEqual(
            len(states_named("stop")), 1, "the final turn should leave the speaking state"
        )

        # Barge-in proof: the weather question must interrupt TTS while it was
        # actively synthesizing the joke, not after the joke had finished.
        await asyncio.sleep(0.5)  # let the Python node's flushed log land on disk
        server_log = SERVER_LOG.read_text(encoding="utf-8", errors="replace")
        self.assertIn(
            "actively_synthesizing=True",
            server_log,
            "barge-in should interrupt an actively synthesizing TTS session",
        )

        print("\n[full-duplex] three-turn summary:")
        for label, heard, said in turn_log:
            print(f"  {label:20s} heard={heard!r}  said={said!r}")
        print(
            "[full-duplex] display states: "
            f"starts={len(states_named('start'))} "
            f"sentences={len(states_named('sentence_start'))} "
            f"stops={len(states_named('stop'))}"
        )


if __name__ == "__main__":
    unittest.main()
