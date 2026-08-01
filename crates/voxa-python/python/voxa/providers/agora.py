"""Bounded integration with the Agora community Python RTC SDK.

The Agora callback only validates and copies PCM16 into a bounded queue. A
Voxa Node or application thread pulls the resulting owned ``AudioFrame``; user
code never runs inline on Agora's callback thread.
"""

from __future__ import annotations

import ctypes
import importlib
import queue
from dataclasses import dataclass
from types import ModuleType
from typing import Optional

from .._native import AudioFrame


@dataclass(frozen=True)
class AgoraIngressStats:
    accepted: int
    full_dropped: int
    invalid_dropped: int
    closed_dropped: int
    queued: int


class AgoraAudioIngress:
    """Copy-only PCM16 ingress used by an Agora ``AudioFrameObserver``."""

    def __init__(self, capacity: int = 32, max_frame_bytes: int = 384_000) -> None:
        if capacity <= 0:
            raise ValueError("capacity must be positive")
        if max_frame_bytes <= 0:
            raise ValueError("max_frame_bytes must be positive")
        self._queue: queue.Queue[AudioFrame] = queue.Queue(maxsize=capacity)
        self._max_frame_bytes = max_frame_bytes
        self._accepted = 0
        self._full = 0
        self._invalid = 0
        self._closed_drops = 0
        self._closed = False
        self._sequence = 0

    def create_observer(self, agora: Optional[ModuleType] = None):
        """Create the SDK observer without importing Agora at package import time."""
        module = agora or _load_agora()
        owner = self

        class Observer(module.AudioFrameObserver):
            def _submit(
                self,
                samples: int,
                bytes_per_sample: int,
                channels: int,
                sample_rate_hz: int,
                address: int,
                timestamp_ms: int,
            ) -> None:
                owner._submit(
                    samples,
                    bytes_per_sample,
                    channels,
                    sample_rate_hz,
                    address,
                    timestamp_ms,
                )

            def onRecordAudioFrame(self, _kind, samples, width, channels, rate, address, timestamp, _avsync):
                return None

            def onPlaybackAudioFrame(self, _kind, samples, width, channels, rate, address, timestamp, _avsync):
                self._submit(samples, width, channels, rate, address, timestamp)

            def onMixedAudioFrame(self, _kind, samples, width, channels, rate, address, timestamp, _avsync):
                return None

            def onPlaybackAudioFrameBeforeMixing(self, _uid, _kind, samples, width, channels, rate, address, timestamp, _avsync):
                return None

        return Observer()

    def _submit(
        self,
        samples: int,
        bytes_per_sample: int,
        channels: int,
        sample_rate_hz: int,
        address: int,
        timestamp_ms: int,
    ) -> None:
        if self._closed:
            self._closed_drops += 1
            return
        if (
            samples <= 0
            or bytes_per_sample != 2
            or channels not in (1, 2)
            or sample_rate_hz not in (8000, 16000, 32000, 44100, 48000)
            or not address
        ):
            self._invalid += 1
            return
        size = samples * channels * bytes_per_sample
        if size > self._max_frame_bytes:
            self._invalid += 1
            return
        try:
            payload = ctypes.string_at(address, size)
            self._sequence += 1
            frame = AudioFrame(
                payload,
                sample_rate_hz,
                channels,
                samples,
                sample_format_name="i16le",
                layout="interleaved",
                timestamp_ns=timestamp_ms * 1_000_000,
                sequence=self._sequence,
            )
            self._queue.put_nowait(frame)
            self._accepted += 1
        except queue.Full:
            self._full += 1
        except (OverflowError, TypeError, ValueError):
            self._invalid += 1

    def try_pop(self) -> Optional[AudioFrame]:
        try:
            return self._queue.get_nowait()
        except queue.Empty:
            return None

    def close(self) -> bool:
        if self._closed:
            return False
        self._closed = True
        return True

    @property
    def stats(self) -> AgoraIngressStats:
        return AgoraIngressStats(
            accepted=self._accepted,
            full_dropped=self._full,
            invalid_dropped=self._invalid,
            closed_dropped=self._closed_drops,
            queued=self._queue.qsize(),
        )


class AgoraRtcClient:
    """Small lifecycle owner for Agora Python SDK 3.4.2.1 audio callbacks."""

    def __init__(
        self,
        app_id: str,
        ingress: Optional[AgoraAudioIngress] = None,
        agora: Optional[ModuleType] = None,
    ) -> None:
        if not app_id:
            raise ValueError("app_id is required")
        self._agora = agora or _load_agora()
        self._ingress = ingress or AgoraAudioIngress()
        self._engine = self._agora.createRtcEngineBridge()
        self._handler = self._agora.RtcEngineEventHandlerBase()
        self._observer = self._ingress.create_observer(self._agora)
        self._closed = False
        self._observer_registered = False
        try:
            _require_ok(self._engine.initEventHandler(self._handler), "initEventHandler")
            _require_ok(
                self._engine.initialize(
                    app_id, None, self._agora.AREA_CODE_GLOB & 0xFFFFFFFF
                ),
                "initialize",
            )
            _require_ok(
                self._engine.setPlaybackAudioFrameParameters(
                    48000,
                    1,
                    self._agora.RAW_AUDIO_FRAME_OP_MODE_READ_ONLY,
                    480,
                ),
                "setPlaybackAudioFrameParameters",
            )
            self._agora.registerAudioFrameObserver(self._engine, self._observer)
            self._observer_registered = True
        except BaseException:
            self._closed = True
            self._ingress.close()
            self._engine.release()
            raise

    @property
    def ingress(self) -> AgoraAudioIngress:
        return self._ingress

    def join(self, channel: str, token: str = "", uid: int = 0) -> None:
        if self._closed:
            raise RuntimeError("AgoraRtcClient is closed")
        if not channel:
            raise ValueError("channel is required")
        _require_ok(self._engine.joinChannel(token, channel, "", uid), "joinChannel")

    def close(self) -> bool:
        if self._closed:
            return False
        self._closed = True
        self._ingress.close()
        try:
            if self._observer_registered:
                self._agora.unregisterAudioFrameObserver(self._engine, self._observer)
                self._observer_registered = False
            self._engine.leaveChannel()
        finally:
            self._engine.release()
        return True

    def __enter__(self) -> "AgoraRtcClient":
        return self

    def __exit__(self, _exc_type, _exc, _traceback) -> None:
        self.close()


def _load_agora() -> ModuleType:
    try:
        return importlib.import_module("agorartc")
    except ImportError as error:
        raise RuntimeError(
            "Agora Python RTC requires agora-python-sdk==3.4.2.1 and CPython 3.9"
        ) from error


def _require_ok(result: int, operation: str) -> None:
    if result != 0:
        raise RuntimeError(f"Agora {operation} failed with code {result}")
