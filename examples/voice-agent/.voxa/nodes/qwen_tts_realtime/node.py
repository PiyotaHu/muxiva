"""Qwen streaming TTS application Node Pack for Voxa."""

from __future__ import annotations

import base64
import json
import os
import re
import uuid
from typing import Any, Callable, Iterable
from urllib.parse import quote

import voxa


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
        self._socket.settimeout(15)

    def send(self, event: dict[str, Any]) -> None:
        self._socket.send(json.dumps(event, separators=(",", ":")))

    def poll(self, maximum: int = 64) -> Iterable[dict[str, Any]]:
        for _ in range(maximum):
            try:
                value = self._socket.recv()
            except (self._websocket.WebSocketTimeoutException, BlockingIOError):
                return
            if value is None:
                return
            event = json.loads(value)
            if not isinstance(event, dict) or not isinstance(event.get("type"), str):
                raise RuntimeError("Qwen TTS event requires a string type")
            yield event
            if event["type"] == "response.done":
                return

    def close(self) -> None:
        self._socket.close()


class QwenTtsRealtimeNode:
    def __init__(
        self,
        config: dict[str, Any] | None = None,
        transport_factory: Callable[[str, str, dict[str, Any]], Any] = _WebSocketTransport,
    ) -> None:
        self.config = config or {}
        self._factory = transport_factory
        self._transport: Any | None = None

    def on_prepare(self, _ctx: Any = None) -> None:
        key, workspace = _credentials()
        model = str(self.config.get("model", "qwen3-tts-flash-realtime"))
        endpoint = (
            f"wss://{workspace}.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime"
            f"?model={quote(model, safe='-._')}"
        )
        self._transport = self._factory(endpoint, key, session_update(self.config))

    def on_process(self, frame: Any, ctx: Any) -> None:
        if self._transport is None:
            raise RuntimeError("Qwen TTS transport is not prepared")
        if not frame.text:
            return
        self._transport.send({
            "event_id": _event_id(),
            "type": "input_text_buffer.append",
            "text": frame.text,
        })
        self._transport.send({"event_id": _event_id(), "type": "input_text_buffer.commit"})
        for event in self._transport.poll():
            if event["type"] == "response.audio.delta":
                pcm = base64.b64decode(event.get("delta", ""), validate=True)
                if pcm and len(pcm) <= 256 * 1024 and len(pcm) % 2 == 0:
                    ctx.emit("audio_out", voxa.AudioFrame(
                        pcm, sample_rate_hz=24_000, channels=1, sequence=frame.sequence
                    ))
            elif event["type"] == "error":
                error = event.get("error", {})
                raise RuntimeError(f"Qwen TTS: {str(error.get('message', 'request failed'))[:512]}")

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
            "voice": config.get("voice", "Cherry"),
            "mode": "commit",
            "language_type": config.get("language_type", "Auto"),
            "response_format": "pcm",
            "sample_rate": 24_000,
        },
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
    return f"event_voxa_{uuid.uuid4().hex}"
