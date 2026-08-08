# D08: bounded media and codec foundation

## Outcome

D08 adds `Muxiva::media`, an optional C++17 media-normalization layer. The public
pipeline builds without FFmpeg; `MUXIVA_ENABLE_FFMPEG=ON` supplies the real
libswresample/libswscale backend. FFmpeg remains outside Muxiva Core and outside
the default dependency graph.

## V1 formats

- Packed/interleaved PCM: U8, I16LE, I32LE, F32LE and F64LE through FFmpeg.
- I24LE remains part of the Muxiva frame contract but the FFmpeg backend rejects
  it explicitly instead of silently widening samples.
- Video: tightly packed RGBA8 and tightly sequenced I420.
- Audio sample rate, sample format and channel count can change across a
  conversion, including mono/stereo downmix and upmix.
- Video size and pixel format can change with bilinear scaling.

## Bounded execution

Each `Pipeline` has an exact `max_frame_bytes` budget. Input is validated before
the backend runs; output capacity is checked before allocation and validated
again after the backend returns. A Pipeline admits one conversion at a time and
rejects concurrent calls as `busy`, so a realtime callback cannot accidentally
create an unbounded work queue. Applications use one Pipeline per media stream.

The returned frame owns its bytes. The backend never retains a caller view.
V1 uses bounded owned output vectors rather than a reusable cross-ABI buffer
pool; a lease-based pool can be added later without putting allocator ownership
into the C ABI.

## Streaming and time

The FFmpeg resampler is stateful across input frames and must be flushed at end
of stream. Once flush begins, new input is rejected until `reset()`. Format
changes also require reset, preventing delayed samples from being discarded.

Audio output timestamps are sample-derived. Small input jitter within the
configured tolerance snaps to the exact next sample boundary; larger jumps are
preserved and counted as discontinuities. Video timestamps are preserved. D08
does not invent comparability across Muxiva clock domains and does not implement
a player-side A/V jitter buffer.

## Deliberate boundary

D08 normalizes raw media. Compressed packet decode/encode, hardware acceleration,
device capture, AEC/NS/AGC and a cross-stream playback jitter buffer remain
separate provider/node work. Keeping those out of this API prevents codec state
and device threads from leaking into Core.

