"""Generate the simulated Xiaozhi user-voice fixtures with Qwen TTS.

The full-duplex test streams these files into the server as if they were the
ESP32 microphone. They are synthesized with Qwen ``qwen3-tts-flash-realtime``
at 16 kHz mono PCM so that Qwen ASR recognizes them reliably.

Usage (from the repository root):

    DASHSCOPE_API_KEY=... python3 examples/xiaozhi-agent/tests/make_fixtures.py

The API key is read from the environment or ``examples/xiaozhi-agent/.env`` and
is never written to disk. Generated WAV files are placed under ``fixtures/``.
"""

from __future__ import annotations

import base64
import json
import os
import pathlib
import sys
import time
import wave

TTS_ENDPOINT = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=qwen3-tts-flash-realtime"
SAMPLE_RATE = 16_000
VOICE = "Cherry"

UTTERANCES = [
    ("user_01_hello.wav", "你好吗？"),
    ("user_02_joke.wav", "给我说一个笑话"),
    ("user_03_weather.wav", "今天天气怎么样"),
]

HERE = pathlib.Path(__file__).resolve().parent
FIXTURES_DIR = HERE / "fixtures"


def load_api_key() -> str:
    key = os.environ.get("DASHSCOPE_API_KEY", "").strip()
    if key:
        return key
    env_file = HERE.parent / ".env"
    if env_file.is_file():
        for line in env_file.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("DASHSCOPE_API_KEY="):
                key = line.split("=", 1)[1].strip().strip("'\"")
                if key:
                    return key
    raise SystemExit(
        "DASHSCOPE_API_KEY is required; export it or add it to examples/xiaozhi-agent/.env"
    )


def synthesize(text: str, api_key: str) -> bytes:
    """Return 16 kHz mono PCM for ``text`` using Qwen realtime TTS."""
    try:
        import websocket
    except ImportError as error:
        raise SystemExit("install `websocket-client` first: pip install websocket-client") from error

    socket = websocket.create_connection(
        TTS_ENDPOINT,
        header=[f"Authorization: Bearer {api_key}"],
        timeout=15,
    )
    try:
        socket.send(
            json.dumps(
                {
                    "event_id": "session_1",
                    "type": "session.update",
                    "session": {
                        "voice": VOICE,
                        "mode": "commit",
                        "language_type": "Auto",
                        "response_format": "pcm",
                        "sample_rate": SAMPLE_RATE,
                    },
                },
                separators=(",", ":"),
            )
        )
        socket.send(
            json.dumps(
                {"event_id": "append_1", "type": "input_text_buffer.append", "text": text},
                separators=(",", ":"),
            )
        )
        socket.send(
            json.dumps({"event_id": "commit_1", "type": "input_text_buffer.commit"})
        )
        pcm = bytearray()
        while True:
            try:
                message = socket.recv()
            except websocket.WebSocketTimeoutException:
                break
            if not message:
                break
            event = json.loads(message)
            kind = event.get("type")
            if kind == "response.audio.delta":
                pcm.extend(base64.b64decode(event.get("delta", "")))
            elif kind == "error":
                raise SystemExit(f"Qwen TTS error: {event.get('error')}")
            elif kind in ("response.done", "session.finished"):
                break
    finally:
        socket.close()
    if not pcm:
        raise SystemExit(f"Qwen TTS returned no audio for: {text}")
    return bytes(pcm)


def write_wav(path: pathlib.Path, pcm: bytes) -> None:
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(SAMPLE_RATE)
        wav.writeframes(pcm)


def main() -> None:
    api_key = load_api_key()
    FIXTURES_DIR.mkdir(parents=True, exist_ok=True)
    for filename, text in UTTERANCES:
        path = FIXTURES_DIR / filename
        started = time.time()
        pcm = synthesize(text, api_key)
        write_wav(path, pcm)
        print(
            f"[fixtures] {filename}: text={text!r} "
            f"pcm_bytes={len(pcm)} duration_ms={len(pcm) * 1000 // (SAMPLE_RATE * 2)} "
            f"elapsed={time.time() - started:.2f}s"
        )
    print(f"[fixtures] wrote {len(UTTERANCES)} files to {FIXTURES_DIR}")


if __name__ == "__main__":
    main()
