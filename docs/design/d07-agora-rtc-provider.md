# D07: Agora RTC provider

## Outcome

D07 adds an optional Agora boundary without making a proprietary SDK a Voxa
Core dependency. The C++ provider translates Agora 4.x raw PCM16/I420 callbacks
into the bounded `ExternalIngress`; the Python provider translates the community
Python SDK playback callback into an owned, bounded `AudioFrame` queue.

## Contract

- Agora callback threads may validate, copy, increment bounded counters, and
  attempt non-blocking admission only. They never execute a Node.
- All buffers are copied before an Agora callback returns.
- C++ SDK control calls run on one owned serial thread. `leave()` is idempotent,
  stops admission, unregisters observers, drains callbacks, destroys custom
  tracks, releases the engine, and closes ingress in that order.
- Audio is PCM16 interleaved. The native publishing profile is 48 kHz mono.
- Video is tightly packed I420 at the Voxa ABI and stride-aware I420 at the
  Agora boundary. Dimensions must be even.
- Queue-full, invalid, closed, and late callbacks are observable counters; no
  unbounded retry or hidden queue is allowed.
- Connection, participant, and error callbacks become Voxa Signal/Event frames.

## Packaging boundary

`VOXA_ENABLE_AGORA=OFF` is the default. It builds and tests the public adapter
contract with no Agora files. Enabling it requires an independently obtained
Agora Native SDK root and links the vendor library into `Voxa::agora`.

The Python integration lazily imports `agorartc`; importing `voxa` has no Agora
dependency. The available community wheel is pinned to `agora-python-sdk
3.4.2.1` and CPython 3.9 for its supported macOS binary.

## Verification boundary

The deterministic fake-SDK contract test covers copied audio/video, signals,
bounded admission, outbound custom media, idempotent shutdown, and a deliberately
late callback. ASan/UBSan run the same test. The native implementation is
syntax-checked against current Agora 4.x headers; the real Python extension is
import-probed. Joining a live channel remains a credentialed integration gate
and is intentionally not part of offline CI.

