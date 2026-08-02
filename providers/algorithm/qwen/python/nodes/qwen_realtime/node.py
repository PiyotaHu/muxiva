"""Qwen Audio Realtime Voxa Node Pack.

This is application/provider code. It deliberately has no import or build-time
dependency on the Rust Voxa runtime; the generic Python Host injects ``voxa``.
"""

from __future__ import annotations

import base64
import json
import os
import re
import ssl
import sys
import uuid
from typing import Any, Callable, Iterable
from urllib.parse import quote

import voxa


DEFAULT_MODEL = "qwen-audio-3.0-realtime-flash"
DEFAULT_VOICE = "longanqian"


class QwenProtocolError(RuntimeError):
    pass


class _QwenWebSocket:
    def __init__(self, endpoint: str, api_key: str, session: dict[str, Any]) -> None:
        try:
            import websocket
        except ImportError as error:
            raise RuntimeError(
                "Qwen Node Pack requires `python -m pip install websocket-client`"
            ) from error
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
            yield parse_server_event(value)

    def close(self) -> None:
        self._socket.close()


class QwenAudioRealtimeNode:
    def __init__(
        self,
        config: dict[str, Any] | None = None,
        transport_factory: Callable[[str, str, dict[str, Any]], Any] = _QwenWebSocket,
    ) -> None:
        self.config = config or {}
        self._transport_factory = transport_factory
        self._transport: Any | None = None
        self._response_active = False
        self._audio_frames_sent = 0

    @staticmethod
    def _log(event: str, **fields: Any) -> None:
        detail = " ".join(f"{key}={value}" for key, value in fields.items())
        print(f"[VOXA][QWEN][{event}] {detail}".rstrip(), file=sys.stderr, flush=True)

    def on_prepare(self, _ctx: Any = None) -> None:
        api_key = os.environ.get("DASHSCOPE_API_KEY", "")
        workspace_id = os.environ.get("DASHSCOPE_WORKSPACE_ID", "")
        if not api_key or not workspace_id:
            raise RuntimeError(
                "configure DASHSCOPE_API_KEY and DASHSCOPE_WORKSPACE_ID in Studio Connections"
            )
        if re.fullmatch(r"[A-Za-z0-9-]{1,128}", workspace_id) is None:
            raise ValueError("DASHSCOPE_WORKSPACE_ID has an invalid format")
        model = str(self.config.get("model", DEFAULT_MODEL))
        endpoint = (
            f"wss://{workspace_id}.cn-beijing.maas.aliyuncs.com/"
            f"api-ws/v1/realtime?model={quote(model, safe='-._')}"
        )
        self._transport = self._transport_factory(
            endpoint, api_key, session_update(self.config)
        )
        self._log(
            "session.connect",
            model=model,
            turn_detection=self.config.get("turn_detection", "server_vad"),
            audio="pcm_s16le/16000/mono",
        )

    def on_process(self, frame: Any, ctx: Any) -> None:
        if self._transport is None:
            raise RuntimeError("Qwen realtime transport is not prepared")
        if frame.sample_rate_hz != 16_000 or frame.channels != 1:
            raise ValueError("Qwen input must be mono PCM s16le at 16000 Hz")
        self._transport.send(audio_append(frame.data))
        self._audio_frames_sent += 1
        if self._audio_frames_sent == 1 or self._audio_frames_sent % 500 == 0:
            self._log("audio.sent", frames=self._audio_frames_sent, bytes=len(frame.data))
        for event in self._transport.poll():
            kind = event["type"]
            if kind in {
                "session.created",
                "session.updated",
                "input_audio_buffer.speech_started",
                "input_audio_buffer.speech_stopped",
                "input_audio_buffer.committed",
                "response.created",
                "response.done",
                "error",
            }:
                self._log("event", type=kind)
            if kind == "response.created":
                self._response_active = True
            elif kind == "response.done":
                self._response_active = False
            elif kind == "input_audio_buffer.speech_started":
                if self._response_active:
                    self._transport.send(response_cancel())
                    self._response_active = False
                ctx.emit_signal("voxa.runtime.interrupt", {"provider": "qwen"})
            elif kind == "response.audio.delta":
                audio = base64.b64decode(event["delta"], validate=True)
                if not audio or len(audio) > 256 * 1024 or len(audio) % 2:
                    raise QwenProtocolError("invalid Qwen response audio size")
                ctx.emit(
                    "audio_out",
                    voxa.AudioFrame(audio, sample_rate_hz=24_000, channels=1, sequence=frame.sequence),
                )
            elif kind in ("response.audio_transcript.delta", "response.text.delta"):
                text = event.get("delta", "")
                if text:
                    ctx.emit("text_out", voxa.TextFrame(text, sequence=frame.sequence))
                    ctx.publish_event("voxa.voice.response.delta", {"text": text})
            elif kind == "conversation.item.input_audio_transcription.delta":
                text = event.get("text", "")
                if text:
                    ctx.emit("text_out", voxa.TextFrame(text, sequence=frame.sequence))
                    ctx.publish_event("voxa.voice.transcript.delta", {"text": text})
            elif kind == "error":
                error = event.get("error", {})
                raise QwenProtocolError(
                    f"Qwen provider error {str(error.get('code', 'unknown'))[:128]}: "
                    f"{str(error.get('message', 'request failed'))[:512]}"
                )

    def on_finish(self, _ctx: Any = None) -> None:
        if self._transport is not None:
            self._transport.close()
            self._transport = None

    def on_abort(self, _reason: str, _ctx: Any = None) -> None:
        self.on_finish(_ctx)


def session_update(config: dict[str, Any]) -> dict[str, Any]:
    detection = str(config.get("turn_detection", "server_vad"))
    turn_detection: dict[str, Any] = {"type": detection}
    if detection == "server_vad":
        turn_detection.update(
            threshold=float(config.get("vad_threshold", 0.5)),
            silence_duration_ms=int(config.get("silence_duration_ms", 800)),
        )
    return {
        "event_id": _event_id(),
        "type": "session.update",
        "session": {
            "modalities": ["text", "audio"],
            "voice": config.get("voice", DEFAULT_VOICE),
            "instructions": config.get(
                "instructions", "You are a concise, helpful realtime voice assistant."
            ),
            "input_audio_format": "pcm",
            "output_audio_format": "pcm",
            "turn_detection": turn_detection,
        },
    }


def audio_append(pcm: bytes) -> dict[str, Any]:
    if not pcm or len(pcm) > 256 * 1024:
        raise ValueError("audio chunk must contain 1 byte through 256 KiB")
    return {
        "event_id": _event_id(),
        "type": "input_audio_buffer.append",
        "audio": base64.b64encode(pcm).decode("ascii"),
    }


def response_cancel() -> dict[str, str]:
    return {"event_id": _event_id(), "type": "response.cancel"}


def _event_id() -> str:
    return f"event_voxa_{uuid.uuid4().hex}"


def parse_server_event(value: str | bytes) -> dict[str, Any]:
    if len(value) > 8 * 1024 * 1024:
        raise QwenProtocolError("Qwen server event exceeds 8 MiB")
    event = json.loads(value)
    if not isinstance(event, dict) or not isinstance(event.get("type"), str):
        raise QwenProtocolError("Qwen server event requires a string type")
    return event
