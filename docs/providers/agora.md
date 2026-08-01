# Agora provider setup

Agora is optional. Voxa does not download, redistribute, or silently accept the
license of either SDK.

## C++ Native SDK

Obtain the Agora Native SDK for the target platform, then configure:

```sh
cmake -S providers/agora/cpp -B build/agora \
  -DVOXA_ENABLE_AGORA=ON \
  -DVOXA_AGORA_SDK_ROOT=/absolute/path/to/agora-sdk \
  -DVOXA_SOURCE_ROOT="$PWD"
cmake --build build/agora --target voxa_agora
```

The SDK root must contain `IAgoraRtcEngine.h` under `include` or `sdk/include`,
and `agora_rtc_sdk`/`AgoraRtcKit` under a supported library directory. Link an
application to `VoxaAgora::agora` and the installed Voxa C++ runtime, create an
external ingress, then pass
`make_native_sdk()` to `RtcAdapter::create`.

The implementation uses custom audio and video tracks, publishes PCM16 mono at
48 kHz and I420 video, and receives per-user pre-mixing audio plus remote I420
render frames. The vendor runtime library must be packaged according to Agora's
platform instructions. A header compile is not a substitute for a binary test
on the intended Linux/Windows deployment target.

`RtcAdapter::renew_token` updates credentials without rebuilding the engine.
Connection callbacks expose epochs/reconnect counts, and bounded control frames
cover token warnings, connection loss, network quality, and call statistics.
The adapter never copies a token into a frame or metrics snapshot.

## Language boundary

Agora integration is C++-only. Its headers, implementation, build project, and
tests live under `providers/agora/cpp`; the root Voxa CMake project and Python
package contain no Agora target or SDK wrapper.
The flagship application's C++ Node Pack is under
`examples/voice-agent/.voxa/nodes/agora_*`. Studio discovers its Manifest but
does not compile or link Agora itself.

Do not commit App IDs, certificates, or tokens. Studio's generic Connection
store passes the Manifest-declared environment variables only to the owning
Node process. See [`D09 Agora production readiness`](../design/d09-agora-production-readiness.md).

## Live acceptance checklist

1. Join with two clients and verify participant and connection events.
2. Receive 48 kHz PCM16 and I420 for ten minutes with bounded queue metrics.
3. Publish Voxa PCM16/I420 custom tracks and verify remote playout/render.
4. Disconnect/reconnect, rotate a short-lived token through the token file,
   then close during active callbacks.
5. Confirm no callback after `leave()` touches freed state and no vendor thread
   remains after shutdown.
