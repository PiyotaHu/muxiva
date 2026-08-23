"""Xiaozhi audio egress Sink Node.

Receives TTS PCM Audio Frames and publishes them to the shared gateway, which
encodes them to Opus and streams them back to the device. A barge-in Signal
clears any queued assistant audio.
"""

from __future__ import annotations

import json
import os
import sys

_SHARED = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
if _SHARED not in sys.path:
    sys.path.insert(0, _SHARED)

import xiaozhi_gateway  # noqa: E402


class XiaozhiAudioSinkNode:
    def __init__(self, config: dict | None = None) -> None:
        self.config = config or {}
        self._client = xiaozhi_gateway.XiaozhiControlClient(
            host=str(self.config.get("control_host", "127.0.0.1")),
            port=int(self.config.get("control_port", 8889)),
            role="sink",
        )
        self._closed = False
        self._cancelled_through_sequence = 0
        self._received_audio_frames: dict[int, int] = {}
        self._pending_stop: tuple[int, int] | None = None
        self._stopped_through_sequence = -1

    def on_prepare(self, ctx) -> None:
        self._connect()

    def on_process(self, frame, ctx) -> None:
        if self._closed:
            raise RuntimeError("Xiaozhi audio sink is closed")
        if frame is None:
            return
        input_port = getattr(ctx, "input_port", "audio_in") if ctx is not None else "audio_in"
        if input_port == "event_in":
            self._handle_event(frame)
            return
        if input_port not in (None, "audio_in"):
            raise ValueError(f"unsupported Xiaozhi audio sink input Port: {input_port}")
        # Signals are delivered out of band and can overtake PCM Frames that
        # were already queued between TTS -> resampler -> sink. Clearing the
        # gateway alone is therefore insufficient: stale Frames can arrive a
        # moment later and refill it, producing a skipped middle followed by
        # an old audio tail. Keep the cancellation watermark at the final
        # transport boundary. The new transcript intentionally shares the
        # Signal sequence, so only strictly older audio is stale.
        if int(getattr(frame, "sequence", 0)) < self._cancelled_through_sequence:
            return
        if not self._client.is_connected() and not self._connect():
            return
        data = bytes(getattr(frame, "data", b""))
        if data:
            self._client.send({"op": "audio", "pcm_hex": data.hex()})
            sequence = int(getattr(frame, "sequence", 0))
            self._received_audio_frames[sequence] = (
                self._received_audio_frames.get(sequence, 0) + 1
            )
            self._maybe_send_stop(sequence)

    def _handle_event(self, frame) -> None:
        if getattr(frame, "topic", "") != "muxiva.voice.tts.drained":
            return
        sequence = int(getattr(frame, "sequence", 0))
        if sequence < self._cancelled_through_sequence:
            return
        try:
            payload = json.loads(getattr(frame, "payload", "") or "{}")
        except (TypeError, json.JSONDecodeError):
            payload = {}
        expected = max(0, int(payload.get("audio_frames", 0)))
        self._pending_stop = (sequence, expected)
        print(
            "[MUXIVA][XIAOZHI][media_barrier.armed] "
            f"sequence={sequence} expected_frames={expected} "
            f"received_frames={self._received_audio_frames.get(sequence, 0)}",
            file=sys.stderr,
            flush=True,
        )
        self._maybe_send_stop(sequence)

    def _maybe_send_stop(self, sequence: int) -> None:
        pending = self._pending_stop
        if pending is None or pending[0] != sequence:
            return
        expected = pending[1]
        received = self._received_audio_frames.get(sequence, 0)
        if received < expected or sequence <= self._stopped_through_sequence:
            return
        if not self._client.is_connected() and not self._connect():
            return
        self._client.send({
            "op": "message",
            "payload": {"type": xiaozhi_gateway.TTS_TYPE, "state": "stop"},
        })
        print(
            "[MUXIVA][XIAOZHI][media_barrier.released] "
            f"sequence={sequence} expected_frames={expected} "
            f"received_frames={received}",
            file=sys.stderr,
            flush=True,
        )
        self._stopped_through_sequence = sequence
        self._pending_stop = None
        self._received_audio_frames = {
            key: value
            for key, value in self._received_audio_frames.items()
            if key > sequence
        }

    def on_signal(self, signal, ctx=None) -> None:
        name = getattr(signal, "name", "")
        if name not in {
            "muxiva.turn.cancelled",
            "muxiva.voice.speech.started",  # pre-controller compatibility
        }:
            return
        # Drop queued assistant audio when the user barges in.
        stop_was_waiting_for_tail = self._pending_stop is not None
        self._cancelled_through_sequence = max(
            self._cancelled_through_sequence,
            int(getattr(signal, "sequence", 0)),
        )
        self._pending_stop = None
        self._received_audio_frames = {
            key: value
            for key, value in self._received_audio_frames.items()
            if key >= self._cancelled_through_sequence
        }
        self._client.send({"op": "reset"})
        # Once the TTS-drained event has moved normal stop ownership to this
        # sink, a barge-in in that narrow barrier window must also take the
        # device out of speaking state after clearing its media queue.
        if stop_was_waiting_for_tail:
            self._client.send({
                "op": "message",
                "payload": {"type": xiaozhi_gateway.TTS_TYPE, "state": "stop"},
            })

    def _connect(self) -> bool:
        return self._client.connect()

    def on_finish(self, ctx=None) -> None:
        self._closed = True
        self._client.close()

    def on_abort(self, reason: str, ctx=None) -> None:
        self.on_finish(ctx)
