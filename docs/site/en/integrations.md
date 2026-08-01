# Providers

Providers connect Voxa to RTC SDKs, media libraries, model APIs, transports, and
devices. They remain outside Runtime Core and must preserve bounded ingress,
ownership, cancellation, and shutdown contracts.

## Agora

The repository contains:

- an in-memory Mock RTC contract;
- an optional Agora C++ adapter for PCM16 audio and I420 video;
- an optional Python audio provider;
- scripts for credentialed live-room and soak testing.

Live credentials are never committed. Production certification still requires
retained per-platform live-room evidence, long-duration tests, reconnect and
late-callback faults, and release-specific SDK compatibility.

## Flagship voice profile: Agora + Qwen

The live Voice Playground architecture uses Agora Web SDK for browser microphone and
playback transport. A Voxa Bot joins the same room through the Native adapter,
receives per-user PCM, and sends generated PCM through a custom audio track.

Alibaba Cloud Model Studio Qwen Audio Realtime is the first intelligence
provider profile. One server-side WebSocket supplies turn detection,
transcription, reasoning, streaming speech, and interruption events. Voxa still
owns typed Frames, `TurnId`, cancellation, stale-output filtering, bounded
queues, routing, and metrics. The browser never receives a DashScope API key or
Agora App Certificate.

The deterministic scripted graph is a CI simulation, not a substitute for this
credentialed live path. See the D10 design record in the repository for the
media, configuration, and interruption contracts.

Studio provides a **Connections** dialog for DashScope and Agora. Secret values
are password inputs, are cleared after submission, remain only in local process
memory for the initial implementation, and are never returned by the status
API or saved with the Graph. The Voice Graph Gallery shows both a recommended
six-Node end-to-end Realtime topology and an inspectable ten-Node VAD → ASR →
LLM → TTS cascade. Applying either graph remains disabled until all of its exact
Provider Factories are installed.

The first implementation slice is now executable: `voxa-provider-qwen`
authenticates the Qwen Audio Realtime WebSocket, sends 16 kHz mono PCM, decodes
24 kHz response audio and incremental transcripts, and maps an adjacent Voxa
interrupt Signal to `response.cancel`. Credentials reach the Node only through
the runtime `ResourceStore`. The built-in PCM16 resampler covers both demo
directions. Agora Node wrappers, browser room controls, and the separate
cascade Providers remain the next integration slices; Studio keeps those
templates disabled until those exact Factories exist.

## FFmpeg

The optional media layer provides streaming audio resampling and video scale or
color conversion, including RGBA8 and I420 paths. FFmpeg remains an optional
provider dependency rather than a Core requirement.

## Provider acceptance checklist

A provider proposal must define:

- supported SDK versions, platforms, architectures, and licenses;
- input/output Frame schemas and clock behavior;
- callback threads and ownership transfer;
- queue and byte bounds, backpressure, and overflow behavior;
- cancellation, reconnect, late callback, and shutdown behavior;
- mock, fault-injection, live, and soak test strategy;
- metrics, diagnostics, credentials, and secret handling.
