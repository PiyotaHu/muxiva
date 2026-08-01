# Agora provider setup

Agora is optional. Voxa does not download, redistribute, or silently accept the
license of either SDK.

## C++ Native SDK

Obtain the Agora Native SDK for the target platform, then configure:

```sh
cmake -S . -B build/agora \
  -DVOXA_ENABLE_AGORA=ON \
  -DVOXA_AGORA_SDK_ROOT=/absolute/path/to/agora-sdk
cmake --build build/agora --target voxa_agora
```

The SDK root must contain `IAgoraRtcEngine.h` under `include` or `sdk/include`,
and `agora_rtc_sdk`/`AgoraRtcKit` under a supported library directory. Link an
application to `Voxa::agora`, create an external ingress, then pass
`make_native_sdk()` to `RtcAdapter::create`.

The implementation uses custom audio and video tracks, publishes PCM16 mono at
48 kHz and I420 video, and receives per-user pre-mixing audio plus remote I420
render frames. The vendor runtime library must be packaged according to Agora's
platform instructions. A header compile is not a substitute for a binary test
on the intended Linux/Windows deployment target.

## Python SDK

The community package has a narrow binary compatibility window. Use CPython 3.9:

```sh
python3.9 -m venv .venv-agora
.venv-agora/bin/python -m pip install agora-python-sdk==3.4.2.1 voxa
VOXA_AGORA_PYTHON=.venv-agora/bin/python ./scripts/check-agora-python.sh
```

Run the checked-in audio example with short-lived credentials:

```sh
export VOXA_AGORA_APP_ID='...'
export VOXA_AGORA_TOKEN='...'
export VOXA_AGORA_CHANNEL='voxa-test'
.venv-agora/bin/python examples/python/agora_audio.py
```

Do not commit App IDs, certificates, or tokens. The example only receives
playback audio into a bounded queue; it does not execute application code on an
Agora callback thread.

## Live acceptance checklist

1. Join with two clients and verify participant and connection events.
2. Receive 48 kHz PCM16 and I420 for ten minutes with bounded queue metrics.
3. Publish Voxa PCM16/I420 custom tracks and verify remote playout/render.
4. Disconnect/reconnect, expire the token, then close during active callbacks.
5. Confirm no callback after `leave()` touches freed state and no vendor thread
   remains after shutdown.
