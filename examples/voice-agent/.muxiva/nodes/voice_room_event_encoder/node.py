"""Voice Room application protocol Node.

This Node intentionally lives inside the voice-agent project. It maps Muxiva's
generic EventFrame into the browser contract used by this example; neither the
schema nor its voice-specific cancellation policy belongs to Runtime Core.
"""

from __future__ import annotations

import json
from typing import Any

import muxiva


MEDIA_TYPE = "application/vnd.muxiva.client-event+json"
PROTOCOL_VERSION = "muxiva.client-event/v1"
TEXT_TOPICS = {
    "transcript_preview_in": "muxiva.voice.transcript.preview",
    "transcript_in": "muxiva.voice.transcript.completed",
    "response_text_in": "muxiva.voice.response.delta",
}
AGENT_TOPICS = {
    "muxiva.agent.response.started": "muxiva.voice.response.started",
    "muxiva.agent.response.completed": "muxiva.voice.response.completed",
    "muxiva.agent.response.failed": "muxiva.voice.response.failed",
    "muxiva.agent.response.cancelled": "muxiva.voice.response.cancelled",
}


class VoiceRoomEventEncoderNode:
    def __init__(self, _config: dict[str, Any] | None = None) -> None:
        self._cancelled_through_sequence = 0

    def on_process(self, frame: Any, ctx: Any) -> None:
        input_port = ctx.input_port
        if input_port == "event_in":
            topic = AGENT_TOPICS.get(frame.topic, frame.topic)
            payload = frame.payload
            source = frame.source
        else:
            try:
                topic = TEXT_TOPICS[input_port]
            except KeyError as error:
                raise ValueError(f"unsupported Voice Room input Port: {input_port}") from error
            payload = {"text": frame.text}
            source = "voice_room.event_encoder"

        if topic.startswith("muxiva.voice.response.") and (
            frame.sequence <= self._cancelled_through_sequence
        ):
            return

        if isinstance(payload, str):
            try:
                payload = json.loads(payload)
            except json.JSONDecodeError:
                pass

        message = {
            "version": PROTOCOL_VERSION,
            "type": topic,
            "source": source,
            "stream_id": frame.stream_id,
            "trace_id": frame.trace_id,
            "sequence": frame.sequence,
            "timestamp_ns": frame.timestamp_ns,
            "payload": payload,
        }
        encoded = json.dumps(
            message, separators=(",", ":"), ensure_ascii=False
        ).encode("utf-8")
        ctx.emit(
            "message_out",
            muxiva.ByteFrame(encoded, media_type=MEDIA_TYPE, sequence=frame.sequence),
        )

    def on_signal(self, signal: Any, _ctx: Any = None) -> None:
        self._cancelled_through_sequence = max(
            self._cancelled_through_sequence, signal.sequence
        )
