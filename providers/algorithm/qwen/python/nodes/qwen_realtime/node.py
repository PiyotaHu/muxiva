"""Qwen Audio Realtime Muxiva Node Pack.

This is application/provider code. It deliberately has no import or build-time
dependency on the Rust Muxiva runtime; the generic Python Host injects ``muxiva``.
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

import muxiva


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
        self._cancel_pending = False
        self._discard_response_output = False
        self._audio_frames_sent = 0
        self._response_text = ""
        self._response_audio_bytes = 0

    @staticmethod
    def _log(event: str, **fields: Any) -> None:
        detail = " ".join(f"{key}={value}" for key, value in fields.items())
        print(f"[MUXIVA][QWEN][{event}] {detail}".rstrip(), file=sys.stderr, flush=True)

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
                "conversation.item.input_audio_transcription.completed",
                "conversation.item.input_audio_transcription.failed",
                "response.created",
                "response.done",
                "error",
            }:
                self._log("event", type=kind)
            if kind == "response.created":
                self._response_active = True
                self._cancel_pending = False
                self._discard_response_output = False
                self._response_text = ""
                self._response_audio_bytes = 0
            elif kind == "response.done":
                self._response_active = False
                if not self._discard_response_output:
                    self._emit_client_event(
                        ctx,
                        "muxiva.voice.response.completed",
                        {"text": self._response_text, "audio_bytes": self._response_audio_bytes},
                        frame.sequence,
                    )
            elif kind == "input_audio_buffer.speech_started":
                response_cancelled = self._response_active
                if self._response_active:
                    self._transport.send(response_cancel())
                    self._response_active = False
                    self._cancel_pending = True
                    self._discard_response_output = True
                ctx.emit_signal("muxiva.voice.speech.started", {"node": "qwen.audio_realtime"})
                self._emit_client_event(
                    ctx, "muxiva.voice.speech.started", {"node": "qwen.audio_realtime"}, frame.sequence
                )
                if response_cancelled:
                    self._emit_client_event(
                        ctx,
                        "muxiva.voice.barge_in",
                        {"node": "qwen.audio_realtime", "response_cancelled": True},
                        frame.sequence,
                    )
            elif kind == "input_audio_buffer.speech_stopped":
                self._emit_client_event(
                    ctx, "muxiva.voice.speech.stopped", {"node": "qwen.audio_realtime"}, frame.sequence
                )
            elif kind == "response.audio.delta":
                if self._discard_response_output:
                    continue
                audio = base64.b64decode(event["delta"], validate=True)
                if not audio or len(audio) > 256 * 1024 or len(audio) % 2:
                    raise QwenProtocolError("invalid Qwen response audio size")
                self._response_audio_bytes += len(audio)
                ctx.emit(
                    "audio_out",
                    muxiva.AudioFrame(audio, sample_rate_hz=24_000, channels=1, sequence=frame.sequence),
                )
            elif kind in ("response.audio_transcript.delta", "response.text.delta"):
                if self._discard_response_output:
                    continue
                text = event.get("delta", "")
                if text:
                    self._response_text += text
                    ctx.emit("response_text_out", muxiva.TextFrame(text, sequence=frame.sequence))
                    self._emit_client_event(
                        ctx, "muxiva.voice.response.delta", {"text": text}, frame.sequence
                    )
            elif kind == "conversation.item.input_audio_transcription.delta":
                text = f"{event.get('text', '')}{event.get('stash', '')}"
                if text:
                    ctx.emit("transcript_preview_out", muxiva.TextFrame(text, sequence=frame.sequence))
                    self._emit_client_event(
                        ctx, "muxiva.voice.transcript.preview", {"text": text}, frame.sequence
                    )
            elif kind == "conversation.item.input_audio_transcription.completed":
                text = event.get("transcript", "")
                if text:
                    ctx.emit("transcript_out", muxiva.TextFrame(text, sequence=frame.sequence))
                    self._emit_client_event(
                        ctx, "muxiva.voice.transcript.completed", {"text": text}, frame.sequence
                    )
            elif kind == "conversation.item.input_audio_transcription.failed":
                error = event.get("error", {})
                self._emit_client_event(
                    ctx,
                    "muxiva.voice.transcript.failed",
                    {"message": str(error.get("message", "ASR transcription failed"))[:512]},
                    frame.sequence,
                )
            elif kind == "error":
                error = event.get("error", {})
                code = str(error.get("code", "unknown"))[:128]
                message = str(error.get("message", "request failed"))[:512]
                self._log("node.error", code=code, message=json.dumps(message))
                if self._cancel_pending and _is_cancel_race(code, message):
                    self._cancel_pending = False
                    self._log("cancel.race", action="ignored", reason="response_already_done")
                    continue
                raise QwenProtocolError(
                    f"Qwen Node error {code}: {message}"
                )

    @staticmethod
    def _emit_client_event(ctx: Any, topic: str, payload: dict[str, Any], sequence: int) -> None:
        ctx.emit(
            "client_event_out",
            muxiva.EventFrame(
                topic,
                json.dumps(payload, separators=(",", ":"), ensure_ascii=False),
                source="qwen.audio_realtime",
                sequence=sequence,
            ),
        )
        ctx.publish_notification(topic, payload)

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
            threshold=float(config.get("vad_threshold", 0.35)),
            silence_duration_ms=int(config.get("silence_duration_ms", 1000)),
        )
    instructions = str(
        config.get("instructions", "You are a concise, helpful realtime voice assistant.")
    ).strip()
    instructions += (
        "\nRealtime voice rules: reply in at most two short spoken sentences unless the "
        "user explicitly asks for detail. Respond promptly. If a request needs missing "
        "information, such as a weather location, ask one concise follow-up question."
    )
    return {
        "event_id": _event_id(),
        "type": "session.update",
        "session": {
            "modalities": ["text", "audio"],
            "voice": config.get("voice", DEFAULT_VOICE),
            "instructions": instructions,
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


def _is_cancel_race(code: str, message: str) -> bool:
    detail = f"{code} {message}".lower()
    return "cancel" in detail and any(
        marker in detail
        for marker in (
            "no active",
            "not active",
            "not found",
            "already done",
            "already completed",
            "cannot cancel",
        )
    )


def _event_id() -> str:
    return f"event_muxiva_{uuid.uuid4().hex}"


def parse_server_event(value: str | bytes) -> dict[str, Any]:
    if len(value) > 8 * 1024 * 1024:
        raise QwenProtocolError("Qwen server event exceeds 8 MiB")
    event = json.loads(value)
    if not isinstance(event, dict) or not isinstance(event.get("type"), str):
        raise QwenProtocolError("Qwen server event requires a string type")
    return event
