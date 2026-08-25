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
INPUT_SAMPLE_RATE_HZ = 16_000
INPUT_CHANNELS = 1
INPUT_SAMPLE_WIDTH_BYTES = 2
DEFAULT_INPUT_CHUNK_MS = 100


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
        self._pending_events: list[dict[str, Any]] = []
        try:
            created = self._receive_until("session.created")
            self._socket.send(json.dumps(session, separators=(",", ":")))
            updated = self._receive_until("session.updated")
        except Exception:
            self._socket.close()
            raise
        self._pending_events.extend((created, updated))
        self._socket.settimeout(0)

    def _receive_until(self, expected_type: str, maximum: int = 64) -> dict[str, Any]:
        for _ in range(maximum):
            try:
                value = self._socket.recv()
            except self._websocket.WebSocketTimeoutException as error:
                raise QwenProtocolError(
                    f"timed out waiting for Qwen {expected_type}"
                ) from error
            if value is None or value == b"" or value == "":
                raise QwenProtocolError(
                    f"Qwen connection closed before {expected_type}"
                )
            event = parse_server_event(value)
            if event["type"] == "error":
                detail = event.get("error", {})
                code = str(detail.get("code", "unknown"))[:128]
                message = str(detail.get("message", "session setup failed"))[:512]
                raise QwenProtocolError(f"Qwen session error {code}: {message}")
            if event["type"] == expected_type:
                return event
            self._pending_events.append(event)
        raise QwenProtocolError(f"Qwen did not send {expected_type} within {maximum} events")

    def send(self, event: dict[str, Any]) -> None:
        self._socket.send(json.dumps(event, separators=(",", ":")))

    def poll(self, maximum: int = 64) -> Iterable[dict[str, Any]]:
        emitted = 0
        pending_events = getattr(self, "_pending_events", [])
        while pending_events and emitted < maximum:
            emitted += 1
            yield pending_events.pop(0)
        for _ in range(maximum - emitted):
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
        self._input_audio = bytearray()
        self._input_frames_received = 0
        self._audio_chunks_sent = 0
        self._audio_bytes_sent = 0
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
        if frame.sample_rate_hz != INPUT_SAMPLE_RATE_HZ or frame.channels != INPUT_CHANNELS:
            raise ValueError("Qwen input must be mono PCM s16le at 16000 Hz")
        self._input_frames_received += 1
        self._input_audio.extend(frame.data)
        self._record_input_metrics(frame.data, ctx)
        chunk_bytes = (
            INPUT_SAMPLE_RATE_HZ
            * INPUT_CHANNELS
            * INPUT_SAMPLE_WIDTH_BYTES
            * int(self.config.get("input_chunk_ms", DEFAULT_INPUT_CHUNK_MS))
            // 1000
        )
        if chunk_bytes <= 0 or chunk_bytes > 64 * 1024:
            raise ValueError("input_chunk_ms must produce a 1 through 65536 byte PCM chunk")
        while len(self._input_audio) >= chunk_bytes:
            chunk = bytes(self._input_audio[:chunk_bytes])
            del self._input_audio[:chunk_bytes]
            self._transport.send(audio_append(chunk))
            self._audio_chunks_sent += 1
            self._audio_bytes_sent += len(chunk)
            increment = getattr(ctx, "increment_counter", None)
            if callable(increment):
                increment("qwen.audio_chunks_sent")
                increment("qwen.audio_bytes_sent", len(chunk))
            if self._audio_chunks_sent == 1 or self._audio_chunks_sent % 50 == 0:
                self._log(
                    "audio.sent",
                    input_frames=self._input_frames_received,
                    chunks=self._audio_chunks_sent,
                    chunk_bytes=len(chunk),
                    total_bytes=self._audio_bytes_sent,
                )
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
                    self._emit_event(
                        ctx,
                        "muxiva.voice.response.completed",
                        {"text": self._response_text, "audio_bytes": self._response_audio_bytes},
                        frame.sequence,
                    )
            elif kind == "input_audio_buffer.speech_started":
                self._speech_started(ctx, frame.sequence)
            elif kind == "input_audio_buffer.speech_stopped":
                self._speech_stopped(ctx, frame.sequence)
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
                    ctx.publish_notification("muxiva.voice.response.delta", {"text": text})
            elif kind == "conversation.item.input_audio_transcription.delta":
                text = f"{event.get('text', '')}{event.get('stash', '')}"
                if text:
                    ctx.emit("transcript_preview_out", muxiva.TextFrame(text, sequence=frame.sequence))
                    ctx.publish_notification("muxiva.voice.transcript.preview", {"text": text})
            elif kind == "conversation.item.input_audio_transcription.completed":
                text = event.get("transcript", "")
                if text:
                    self._log("transcript.completed", text=json.dumps(text, ensure_ascii=False))
                    ctx.emit("transcript_out", muxiva.TextFrame(text, sequence=frame.sequence))
                    ctx.publish_notification("muxiva.voice.transcript.completed", {"text": text})
            elif kind == "conversation.item.input_audio_transcription.failed":
                error = event.get("error", {})
                self._emit_event(
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
                    self._log(
                        "cancel.race",
                        action="ignored",
                        reason="response_already_done",
                        provider_code=code,
                    )
                    continue
                raise QwenProtocolError(
                    f"Qwen Node error {code}: {message}"
                )

    def _speech_started(self, ctx: Any, sequence: int) -> None:
        response_cancelled = self._response_active
        if self._response_active:
            self._transport.send(response_cancel())
            self._response_active = False
            self._cancel_pending = True
            self._discard_response_output = True
            self._log("barge_in", action="cancel_response", sequence=sequence)
        ctx.emit_signal("muxiva.voice.speech.started", {"node": "qwen.audio_realtime"})
        self._emit_event(
            ctx, "muxiva.voice.speech.started", {"node": "qwen.audio_realtime"}, sequence
        )
        if response_cancelled:
            self._emit_event(
                ctx,
                "muxiva.voice.barge_in",
                {"node": "qwen.audio_realtime", "response_cancelled": True},
                sequence,
            )

    def _speech_stopped(self, ctx: Any, sequence: int) -> None:
        self._emit_event(
            ctx, "muxiva.voice.speech.stopped", {"node": "qwen.audio_realtime"}, sequence
        )

    @staticmethod
    def _record_input_metrics(pcm: bytes, ctx: Any) -> None:
        peak = 0
        absolute_sum = 0
        sample_count = len(pcm) // 2
        for offset in range(0, sample_count * 2, 2):
            sample = int.from_bytes(pcm[offset : offset + 2], "little", signed=True)
            magnitude = abs(sample)
            absolute_sum += magnitude
            peak = max(peak, magnitude)
        mean_absolute = absolute_sum // sample_count if sample_count else 0
        gauge = getattr(ctx, "set_gauge", None)
        if callable(gauge):
            gauge("input.audio_peak_pcm16", peak)
            gauge("input.audio_mean_abs_pcm16", mean_absolute)
        increment = getattr(ctx, "increment_counter", None)
        if callable(increment):
            increment("input.audio_frames")
            if peak >= 256:
                increment("input.non_silent_frames")

    @staticmethod
    def _emit_event(ctx: Any, topic: str, payload: dict[str, Any], sequence: int) -> None:
        ctx.emit(
            "event_out",
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
    response_is_gone = any(
        marker in detail
        for marker in (
            "conversation has no active response",
            "no active response",
            "response is not active",
            "response not found",
            "response already done",
            "response already completed",
        )
    )
    # Qwen currently reports a late response.cancel in two forms. Some versions
    # mention cancel explicitly; Audio Realtime may only return
    # `invalid_value: Conversation has no active response.`. The caller also
    # requires an outstanding local cancel, so an unrelated provider error is
    # never swallowed here.
    cancel_request_rejected = "cancel" in detail or code.lower() in {
        "invalid_value",
        "invalid_request_error",
    }
    return response_is_gone and cancel_request_rejected


def _event_id() -> str:
    return f"event_muxiva_{uuid.uuid4().hex}"


def parse_server_event(value: str | bytes) -> dict[str, Any]:
    if len(value) > 8 * 1024 * 1024:
        raise QwenProtocolError("Qwen server event exceeds 8 MiB")
    event = json.loads(value)
    if not isinstance(event, dict) or not isinstance(event.get("type"), str):
        raise QwenProtocolError("Qwen server event requires a string type")
    return event
