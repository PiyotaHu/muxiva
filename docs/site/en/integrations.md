# Providers

Providers connect Voxa to RTC SDKs, media libraries, model APIs, transports, and
devices. They remain outside Runtime Core and must preserve bounded ingress,
ownership, cancellation, and shutdown contracts.

## Agora

The repository contains:

- an in-memory Mock RTC contract;
- an optional Agora C++ adapter for PCM16 audio and I420 video;
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
seven-Node end-to-end Realtime topology and an inspectable eleven-Node VAD → ASR →
LLM → TTS cascade. Python ASR, sentence-streaming LLM, committed streaming TTS,
generic VAD/turn context, and the C++ dynamic Node Pack loader are implemented.
A graph becomes runnable only when every exact Factory is installed.

The project **Voice Room** captures the microphone with Agora Web SDK, subscribes
to bot audio, and visualizes graph, callback, and frame activity. Ingress,
egress, and browser clients use three distinct UIDs and short-lived tokens so
native clients cannot replace one another. Browser code receives only the App
ID, channel, web UID, and short-lived web token explicitly exposed by the
Manifest. DashScope keys, bot tokens, and the App Certificate stay server-side.

Provider code is now strictly application-owned. The Qwen Audio Realtime Node
Pack is Python and lives under `examples/voice-agent`; the Agora transport is
C++ and lives under `providers/agora/cpp` plus the application's C++ Nodes.
Core, Graph builtins, and Studio contain no Qwen, DashScope, or Agora code.
The root CMake project also contains no Agora target; the provider has its own
standalone CMake project and depends one-way on Voxa's public ABI.
Studio discovers generic connection fields and graph templates from project
Manifests. Python protocol tests cover Realtime, ASR, sentence-sized streaming
LLM output, explicitly committed TTS, and `response.cancel`. The C++ gate builds
Node Packs, dynamically loads their ABI, and compiles both templates with
Studio's real Registry.

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
