"""Xiaozhi audio ingress Source Node.

Hosts the Xiaozhi WebSocket device server and emits decoded 16 kHz PCM Audio
Frames plus client lifecycle Events and a barge-in Signal. All network work runs
on background threads; the Muxiva Host thread only drains bounded queues during
Runtime ticks.
"""

from __future__ import annotations

import json
import os
import sys

_SHARED = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
if _SHARED not in sys.path:
    sys.path.insert(0, _SHARED)

import muxiva  # noqa: E402  (Host shim)
import xiaozhi_gateway  # noqa: E402


class XiaozhiAudioSourceNode:
    def __init__(self, config: dict | None = None) -> None:
        self.config = config or {}
        self._gateway = xiaozhi_gateway.XiaozhiGateway(self.config)
        self._sequence = 0
        self._closed = False

    def on_prepare(self, ctx) -> None:
        self._gateway.start()
        print(
            "[MUXIVA][XIAOZHI][gateway.started] "
            f"ws={self._gateway.ws_host}:{self._gateway.ws_port} "
            f"control={self._gateway.control_host}:{self._gateway.control_port} "
            f"sample_rate={self._gateway.sample_rate}",
            file=sys.stderr,
            flush=True,
        )

    def on_process(self, frame, ctx) -> None:
        if self._closed:
            raise RuntimeError("Xiaozhi audio source is closed")
        if frame is not None:
            raise ValueError("Xiaozhi audio source received an input frame")

        for pcm in self._gateway.poll_audio():
            self._sequence += 1
            ctx.emit(
                "audio_out",
                muxiva.AudioFrame(
                    pcm,
                    sample_rate_hz=self._gateway.sample_rate,
                    channels=1,
                    sequence=self._sequence,
                ),
            )

        for event in self._gateway.poll_events():
            message_type = event.get("type")
            if message_type == "abort":
                self._sequence += 1
                ctx.emit(
                    "event_out",
                    muxiva.EventFrame(
                        "muxiva.xiaozhi.client.aborted",
                        json.dumps(event, ensure_ascii=False),
                        source="xiaozhi.audio_source",
                        sequence=self._sequence,
                    ),
                )
                # A device abort is an authoritative request, but only the
                # framework Voice Turn Controller may commit cancellation.
                ctx.emit_signal(
                    "muxiva.turn.interrupt.requested", {"source": "client-abort"}
                )
            else:
                self._sequence += 1
                ctx.emit(
                    "event_out",
                    muxiva.EventFrame(
                        f"muxiva.xiaozhi.client.{message_type}",
                        json.dumps(event, ensure_ascii=False),
                        source="xiaozhi.audio_source",
                        sequence=self._sequence,
                    ),
                )

        ctx.schedule_next_tick(10)

    def on_finish(self, ctx=None) -> None:
        self._closed = True
        self._gateway.stop()

    def on_abort(self, reason: str, ctx=None) -> None:
        self.on_finish(ctx)
