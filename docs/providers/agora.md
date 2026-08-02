# Agora provider setup

Agora is optional. Voxa does not download, redistribute, or silently accept the
license of either SDK.

## Fastest macOS installation

Voxa pins the official Agora macOS SDK `4.6.2`. The downloader obtains the six
RTC Basic XCFrameworks from Agora's official CDN and verifies the SHA-256 values
published by the official Swift package:

```sh
./providers/agora/cpp/download-macos-sdk.sh
./examples/voice-agent/setup.sh
```

Official sources:

- [Agora Voice SDK downloads](https://docs.agora.io/en/api-reference/sdks?product=voice&platform=macos)
- [AgoraRtcEngine macOS 4.6.2 package](https://github.com/AgoraIO/AgoraRtcEngine_macOS/tree/4.6.2)
- [Agora account, App ID, and temporary-token guide](https://docs.agora.io/en/realtime-media/voice/manage-agora-account)

The complete beginner credential walkthrough is on the
[flagship voice guide](../site/en/voice-demo.md).

## Manual C++ Native SDK

Obtain the Agora Native SDK for the target platform, then configure:

```sh
cmake -S providers/agora/cpp -B build/agora \
  -DVOXA_ENABLE_AGORA=ON \
  -DVOXA_AGORA_SDK_ROOT=/absolute/path/to/agora-sdk \
  -DVOXA_SOURCE_ROOT="$PWD"
cmake --build build/agora --target voxa_agora
```

The SDK root must contain either an Agora macOS `AgoraRtcKit.xcframework`, or
`IAgoraRtcEngine.h` under `include`/`sdk/include` plus
`agora_rtc_sdk`/`AgoraRtcKit` under a supported library directory. Link an
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
The flagship C++ source/sink Node Packs and their Manifests are under
`providers/agora/cpp/nodes`. The application references that catalog through
`.voxa/providers.json`; Studio discovers it but does not compile or link Agora
itself. Build the complete flagship packs with:

```sh
./examples/voice-agent/setup.sh
```

Ingress, egress, and browser clients use separate UIDs and short-lived RTC
tokens. The browser receives only explicitly exposed room fields; App
Certificates and server/provider credentials must never enter the browser.

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
