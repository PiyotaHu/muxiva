"""Minimal libopus bindings through ctypes.

The Xiaozhi transport needs only one Opus direction at a time: decode device
microphone packets into PCM and encode assistant PCM back into Opus packets.
Binding ``libopus`` directly avoids a Python wrapper dependency and works on
Raspberry Pi once ``libopus0`` is installed.
"""

from __future__ import annotations

import ctypes
import ctypes.util

OPUS_APPLICATION_VOIP = 2048
OPUS_SET_BITRATE = 4002
OPUS_SET_COMPLEXITY = 4010
OPUS_RESET_STATE = 4028

DEFAULT_SAMPLE_RATE = 16_000
DEFAULT_CHANNELS = 1
DEFAULT_FRAME_DURATION_MS = 60


class OpusError(RuntimeError):
    """Raised when libopus is unavailable or a codec call fails."""


def _load_libopus() -> ctypes.CDLL:
    candidates = [
        ctypes.util.find_library("opus"),
        "libopus.so.0",
        "libopus.so",
        "libopus.dylib",
        "opus.dll",
    ]
    for candidate in candidates:
        if not candidate:
            continue
        try:
            return ctypes.CDLL(candidate)
        except OSError:
            continue
    raise OpusError(
        "libopus is not installed; run `sudo apt-get install -y libopus0`"
    )


_lib = _load_libopus()

_lib.opus_encoder_create.argtypes = [
    ctypes.c_int,
    ctypes.c_int,
    ctypes.c_int,
    ctypes.POINTER(ctypes.c_int),
]
_lib.opus_encoder_create.restype = ctypes.c_void_p
_lib.opus_encoder_destroy.argtypes = [ctypes.c_void_p]
_lib.opus_encoder_destroy.restype = None
_lib.opus_encoder_ctl.argtypes = [ctypes.c_void_p, ctypes.c_int]
_lib.opus_encoder_ctl.restype = ctypes.c_int
_lib.opus_encode.argtypes = [
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_int,
    ctypes.c_void_p,
    ctypes.c_int32,
]
_lib.opus_encode.restype = ctypes.c_int32
_lib.opus_decoder_create.argtypes = [
    ctypes.c_int,
    ctypes.c_int,
    ctypes.POINTER(ctypes.c_int),
]
_lib.opus_decoder_create.restype = ctypes.c_void_p
_lib.opus_decoder_destroy.argtypes = [ctypes.c_void_p]
_lib.opus_decoder_destroy.restype = None
_lib.opus_decode.argtypes = [
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_int32,
    ctypes.c_void_p,
    ctypes.c_int,
    ctypes.c_int,
]
_lib.opus_decode.restype = ctypes.c_int32


def _ctl(encoder: ctypes.c_void_p, request: int, value: int) -> None:
    # opus_encoder_ctl is variadic; ctypes can pass one integer argument after
    # the fixed two without declaring the full varargs list.
    _lib.opus_encoder_ctl(encoder, ctypes.c_int(request), ctypes.c_int(value))


class OpusEncoder:
    """Encodes 16-bit little-endian PCM frames into Opus packets."""

    def __init__(
        self,
        sample_rate: int = DEFAULT_SAMPLE_RATE,
        channels: int = DEFAULT_CHANNELS,
        frame_duration_ms: int = DEFAULT_FRAME_DURATION_MS,
        bitrate: int = 16_000,
    ) -> None:
        self.sample_rate = sample_rate
        self.channels = channels
        self.frame_duration_ms = frame_duration_ms
        self.frame_size = sample_rate * frame_duration_ms // 1000
        error = ctypes.c_int()
        self._encoder = _lib.opus_encoder_create(
            ctypes.c_int(sample_rate),
            ctypes.c_int(channels),
            ctypes.c_int(OPUS_APPLICATION_VOIP),
            ctypes.byref(error),
        )
        if not self._encoder or error.value != 0:
            raise OpusError(f"opus_encoder_create failed with code {error.value}")
        _ctl(self._encoder, OPUS_SET_BITRATE, bitrate)
        _ctl(self._encoder, OPUS_SET_COMPLEXITY, 5)

    def encode(self, pcm: bytes) -> bytes:
        expected = self.frame_size * self.channels * 2
        if len(pcm) < expected:
            pcm = pcm + b"\x00" * (expected - len(pcm))
        elif len(pcm) > expected:
            pcm = pcm[:expected]
        source = ctypes.create_string_buffer(pcm, expected)
        output = ctypes.create_string_buffer(expected)
        written = _lib.opus_encode(
            self._encoder,
            source,
            ctypes.c_int(self.frame_size),
            output,
            ctypes.c_int32(len(output)),
        )
        if written < 0:
            raise OpusError(f"opus_encode failed with code {written}")
        return output.raw[:written]

    def reset(self) -> None:
        _lib.opus_encoder_ctl(self._encoder, ctypes.c_int(OPUS_RESET_STATE))

    def close(self) -> None:
        if self._encoder:
            _lib.opus_encoder_destroy(self._encoder)
            self._encoder = None


class OpusDecoder:
    """Decodes Opus packets into 16-bit little-endian PCM frames."""

    def __init__(
        self,
        sample_rate: int = DEFAULT_SAMPLE_RATE,
        channels: int = DEFAULT_CHANNELS,
        frame_duration_ms: int = DEFAULT_FRAME_DURATION_MS,
    ) -> None:
        self.sample_rate = sample_rate
        self.channels = channels
        self.frame_duration_ms = frame_duration_ms
        self.frame_size = sample_rate * frame_duration_ms // 1000
        error = ctypes.c_int()
        self._decoder = _lib.opus_decoder_create(
            ctypes.c_int(sample_rate),
            ctypes.c_int(channels),
            ctypes.byref(error),
        )
        if not self._decoder or error.value != 0:
            raise OpusError(f"opus_decoder_create failed with code {error.value}")

    def decode(self, packet: bytes) -> bytes:
        pcm = ctypes.create_string_buffer(self.frame_size * self.channels * 2)
        samples = _lib.opus_decode(
            self._decoder,
            packet,
            ctypes.c_int32(len(packet)),
            pcm,
            ctypes.c_int(self.frame_size),
            ctypes.c_int(0),
        )
        if samples < 0:
            raise OpusError(f"opus_decode failed with code {samples}")
        return pcm.raw[: samples * self.channels * 2]

    def close(self) -> None:
        if self._decoder:
            _lib.opus_decoder_destroy(self._decoder)
            self._decoder = None
