"""Xiaozhi device protocol Event Encoder Sink Node.

Maps Muxiva voice graph output into the ``stt`` / ``tts`` JSON messages that the
Xiaozhi device firmware renders on its display and status LEDs. This mapping is
application policy, so it stays in the transport provider rather than in Core.
"""

from __future__ import annotations

import json
import os
import sys

_SHARED = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
if _SHARED not in sys.path:
    sys.path.insert(0, _SHARED)

import xiaozhi_gateway  # noqa: E402


class XiaozhiEventEncoderNode:
    def __init__(self, config: dict | None = None) -> None:
        self.config = config or {}
        self._client = xiaozhi_gateway.XiaozhiControlClient(
            host=str(self.config.get("control_host", "127.0.0.1")),
            port=int(self.config.get("control_port", 8889)),
            role="events",
        )
        self._device_command_topics = {
            str(topic) for topic in self.config.get("device_command_topics", [])
        }
        self._device_command_allowlist = {
            str(command_type)
            for command_type in self.config.get("device_command_allowlist", [])
        }
        self._device_command_message_type = str(
            self.config.get("device_command_message_type", "")
        )
        self._cancelled_through_sequence = 0
        self._speaking = False
        self._closed = False

    def on_prepare(self, ctx) -> None:
        self._client.connect()

    def on_process(self, frame, ctx) -> None:
        if self._closed or frame is None:
            return
        if not self._client.is_connected() and not self._client.connect():
            return
        input_port = ctx.input_port
        if input_port == "transcript_in":
            # The device displays the recognized question, then enters the
            # assistant "speaking" state (matching the Xiaozhi stt -> tts start flow).
            self._send({"type": xiaozhi_gateway.STT_TYPE, "text": frame.text.strip()})
            self._send({"type": xiaozhi_gateway.TTS_TYPE, "state": "start"})
            self._speaking = True
        elif input_port == "response_text_in":
            # A validated transcript and its barge-in Signal intentionally
            # share a sequence.  Only older response text is stale.
            if frame.sequence < self._cancelled_through_sequence:
                return
            self._send(
                {
                    "type": xiaozhi_gateway.TTS_TYPE,
                    "state": "sentence_start",
                    "text": frame.text,
                }
            )
        elif input_port == "event_in":
            if frame.topic in self._device_command_topics:
                try:
                    payload = json.loads(getattr(frame, "payload", "") or "{}")
                except json.JSONDecodeError:
                    return
                command = payload.get("command")
                if not isinstance(command, dict):
                    return
                # Device commands are session output, exactly like TTS text and
                # PCM. They travel over the already-bound device WebSocket;
                # the Agent must never discover or call a device IP address.
                if (
                    not self._device_command_message_type
                    or command.get("type") not in self._device_command_allowlist
                ):
                    print(
                        "[MUXIVA][XIAOZHI][device_command.rejected] "
                        f"command_id={payload.get('command_id', '')} "
                        f"type={command.get('type', '')} reason=not_allowed",
                        file=sys.stderr,
                        flush=True,
                    )
                    return
                print(
                    "[MUXIVA][XIAOZHI][device_command.forwarded] "
                    f"command_id={payload.get('command_id', '')} "
                    f"type={command.get('type', '')}",
                    file=sys.stderr,
                    flush=True,
                )
                self._send(
                    {
                        "type": self._device_command_message_type,
                        "command_id": str(payload.get("command_id", "")),
                        "payload": command,
                    }
                )
            elif frame.topic == "muxiva.agent.emotion.changed":
                try:
                    payload = json.loads(getattr(frame, "payload", "") or "{}")
                except json.JSONDecodeError:
                    payload = {}
                emotion = str(payload.get("emotion", "neutral"))
                if emotion in {
                    "neutral", "happy", "laughing", "sad", "angry",
                    "thinking", "relaxed", "confident",
                }:
                    self._send({"type": "llm", "emotion": emotion})
            elif frame.topic == "muxiva.voice.tts.drained":
                # The audio sink owns the final media barrier.  Event and PCM
                # use independent Graph edges, so sending stop here can overtake
                # tail audio that is still buffered behind the resampler.
                self._speaking = False
            elif frame.topic in (
                "muxiva.agent.response.completed",
                "muxiva.voice.response.completed",
            ):
                # TTS owns the Turn/media drain barrier. Agent completion by
                # itself is too early because synthesis can still be running.
                return
            elif frame.topic in (
                "muxiva.agent.response.failed",
                "muxiva.voice.response.failed",
            ):
                # Failed turns may never reach TTS, so request media shutdown.
                self._send({"type": xiaozhi_gateway.TTS_TYPE, "state": "stop"})
                self._speaking = False
            elif frame.topic in {
                "muxiva.voice.speech.started",
                "muxiva.voice.speech.stopped",
            }:
                # Raw VAD events are observational only.  Echo, coughs and
                # fillers can all produce speech.started before ASR has a
                # meaningful final transcript.  Clearing transport audio here
                # used to delete the middle of the active reply; the remaining
                # tail then refilled the queue and started playing again.
                # Playback cancellation is owned exclusively by on_signal(),
                # which receives only validated barge-in Signals.
                return
        else:
            raise ValueError(f"unsupported Xiaozhi Event input Port: {input_port}")

    def on_signal(self, signal, ctx=None) -> None:
        name = getattr(signal, "name", "")
        if name not in {
            "muxiva.turn.cancelled",
            "muxiva.voice.speech.started",  # pre-controller compatibility
        }:
            return
        self._cancelled_through_sequence = max(
            self._cancelled_through_sequence, signal.sequence
        )
        # A barge-in clears queued audio and, only when the assistant was
        # actually speaking, returns the device display to the listening state.
        self._client.send({"op": "reset"})
        if self._speaking:
            self._send({"type": xiaozhi_gateway.TTS_TYPE, "state": "stop"})
        self._speaking = False

    def _send(self, payload: dict) -> None:
        self._client.send({"op": "message", "payload": payload})

    def on_finish(self, ctx=None) -> None:
        self._closed = True
        self._client.close()

    def on_abort(self, reason: str, ctx=None) -> None:
        self.on_finish(ctx)
