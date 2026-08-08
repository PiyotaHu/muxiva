"""Qwen streaming ASR application Node Pack for Muxiva."""

from __future__ import annotations

import base64
import json
import os
import re
import ssl
import uuid
from typing import Any, Callable, Iterable
from urllib.parse import quote

import muxiva


class QwenAsrProtocolError(RuntimeError):
    pass


class _WebSocketTransport:
    def __init__(self, endpoint: str, api_key: str, session: dict[str, Any]) -> None:
        try:
            import websocket
        except ImportError as error:
            raise RuntimeError("install this Node Pack's requirements.txt") from error
        self._websocket = websocket
        self._socket = websocket.create_connection(
            endpoint,
            header=[f"Authorization: Bearer {api_key}"],
            timeout=10,
            enable_multithread=False,
        )
        self._socket.send(json.dumps(session, separators=(",", ":")))
        self._socket.settimeout(0)

    def send(self, event: dict[str, Any]) -> None:
        self._socket.send(json.dumps(event, separators=(",", ":")))

    def poll(self, maximum: int = 64) -> Iterable[dict[str, Any]]:
        for _ in range(maximum):
            try:
                value = self._socket.recv()
            except (self._websocket.WebSocketTimeoutException, BlockingIOError, ssl.SSLWantReadError):
                return
            if value is None:
                return
            event = json.loads(value)
            if not isinstance(event, dict) or not isinstance(event.get("type"), str):
                raise QwenAsrProtocolError("Qwen ASR event requires a string type")
            yield event

    def close(self) -> None:
        self._socket.close()


class QwenAsrRealtimeNode:
    def __init__(
        self,
        config: dict[str, Any] | None = None,
        transport_factory: Callable[[str, str, dict[str, Any]], Any] = _WebSocketTransport,
    ) -> None:
        self.config = config or {}
        self._transport_factory = transport_factory
        self._transport: Any | None = None

    def on_prepare(self, _ctx: Any = None) -> None:
        key, workspace = _credentials()
        model = str(self.config.get("model", "qwen3-asr-flash-realtime"))
        endpoint = (
            f"wss://{workspace}.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime"
            f"?model={quote(model, safe='-._')}"
        )
        self._transport = self._transport_factory(endpoint, key, session_update(self.config))

    def on_process(self, frame: Any, ctx: Any) -> None:
        if self._transport is None:
            raise RuntimeError("Qwen ASR transport is not prepared")
        if frame.sample_rate_hz != 16_000 or frame.channels != 1:
            raise ValueError("Qwen ASR input must be mono PCM s16le at 16000 Hz")
        self._transport.send(audio_append(frame.data))
        for event in self._transport.poll():
            kind = event["type"]
            if kind.endswith("input_audio_transcription.completed"):
                text = event.get("transcript", event.get("text", "")).strip()
                if text:
                    ctx.emit("text_out", muxiva.TextFrame(text, sequence=frame.sequence))
                    ctx.emit(
                        "client_event_out",
                        muxiva.EventFrame(
                            "muxiva.voice.transcript.completed",
                            json.dumps({"text": text}, separators=(",", ":"), ensure_ascii=False),
                            source="qwen.asr_realtime",
                            sequence=frame.sequence,
                        ),
                    )
                    ctx.publish_event("muxiva.voice.transcript.completed", {"text": text})
            elif kind == "error":
                error = event.get("error", {})
                raise QwenAsrProtocolError(
                    f"Qwen ASR {str(error.get('code', 'unknown'))[:128]}: "
                    f"{str(error.get('message', 'request failed'))[:512]}"
                )

    def on_finish(self, _ctx: Any = None) -> None:
        if self._transport is not None:
            self._transport.send({"event_id": _event_id(), "type": "session.finish"})
            self._transport.close()
            self._transport = None

    def on_abort(self, _reason: str, ctx: Any = None) -> None:
        self.on_finish(ctx)


def session_update(config: dict[str, Any]) -> dict[str, Any]:
    return {
        "event_id": _event_id(),
        "type": "session.update",
        "session": {
            "modalities": ["text"],
            "input_audio_format": "pcm",
            "sample_rate": 16_000,
            "input_audio_transcription": {"language": config.get("language", "zh")},
            "turn_detection": {
                "type": "server_vad",
                "threshold": float(config.get("vad_threshold", 0.2)),
                "silence_duration_ms": int(config.get("silence_duration_ms", 500)),
            },
        },
    }


def audio_append(pcm: bytes) -> dict[str, str]:
    if not pcm or len(pcm) > 256 * 1024 or len(pcm) % 2:
        raise ValueError("audio chunk must be non-empty PCM s16le up to 256 KiB")
    return {
        "event_id": _event_id(),
        "type": "input_audio_buffer.append",
        "audio": base64.b64encode(pcm).decode("ascii"),
    }


def _credentials() -> tuple[str, str]:
    key = os.environ.get("DASHSCOPE_API_KEY", "")
    workspace = os.environ.get("DASHSCOPE_WORKSPACE_ID", "")
    if not key or not workspace:
        raise RuntimeError("configure DashScope in Studio Connections")
    if re.fullmatch(r"[A-Za-z0-9-]{1,128}", workspace) is None:
        raise ValueError("DASHSCOPE_WORKSPACE_ID has an invalid format")
    return key, workspace


def _event_id() -> str:
    return f"event_muxiva_{uuid.uuid4().hex}"
