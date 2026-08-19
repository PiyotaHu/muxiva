"""Xiaozhi audio egress Sink Node.

Receives TTS PCM Audio Frames and publishes them to the shared gateway, which
encodes them to Opus and streams them back to the device. A barge-in Signal
clears any queued assistant audio.
"""

from __future__ import annotations

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

    def on_prepare(self, ctx) -> None:
        self._connect()

    def on_process(self, frame, ctx) -> None:
        if self._closed:
            raise RuntimeError("Xiaozhi audio sink is closed")
        if frame is None:
            return
        if not self._client.is_connected() and not self._connect():
            return
        data = bytes(getattr(frame, "data", b""))
        if data:
            self._client.send({"op": "audio", "pcm_hex": data.hex()})

    def on_signal(self, signal, ctx=None) -> None:
        # Drop queued assistant audio when the user barges in.
        self._client.send({"op": "reset"})

    def _connect(self) -> bool:
        return self._client.connect()

    def on_finish(self, ctx=None) -> None:
        self._closed = True
        self._client.close()

    def on_abort(self, reason: str, ctx=None) -> None:
        self.on_finish(ctx)
