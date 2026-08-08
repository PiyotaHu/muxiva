# D10: Qwen Realtime and Agora flagship voice demo

## Decision

The first live Muxiva voice demo uses two official Node collections:

- Agora RTC transports user media between a browser and the Muxiva Bot.
- Alibaba Cloud Model Studio Qwen Audio Realtime provides acoustic turn
  detection, transcription, reasoning, and streaming speech generation over
  one server-side WebSocket.

The initial low-latency model profile is
`qwen-audio-3.0-realtime-flash` in the Beijing region. Vendor model names,
region endpoints, voice, and instructions are configuration, never Core API.
The deterministic scripted demo remains available only as an explicitly named
simulation and CI fixture.

## Why one realtime model first

A cascade of separately managed ASR, text LLM, and TTS sessions remains an
important replaceable profile, but it introduces three network lifecycles and
additional cancellation boundaries before the product experience is proven.
The Qwen realtime protocol already exposes speech-start/speech-stop,
transcripts, streaming response audio, response completion, and cancellation.
It therefore provides the shortest path to an honest full-duplex demo with one
Model Studio account and API key.

Muxiva Core owns only generic runtime semantics. The Qwen Node owns acoustic or
semantic turn boundaries, remote cancellation, and rejection of late response
chunks. It emits typed Signal/Event Frames; the Agora Sink owns playback queue
clearing. Core routes those opaque messages and records queue/runtime metrics.

## Media path

```text
browser microphone
  -> Agora Web SDK microphone track
  -> Agora room
  -> Muxiva Agora per-user PCM ingress (48 kHz PCM16 mono)
  -> bounded resampler (48 kHz -> 16 kHz)
  -> Qwen Realtime WebSocket
  -> streaming transcript + response audio (24 kHz PCM16 mono)
  -> bounded resampler (24 kHz -> 48 kHz)
  -> Muxiva Agora custom audio track
  -> browser remote-track playback
```

No browser request may contain a DashScope API key or Agora App Certificate.
The browser receives only an App ID, channel, UID, and short-lived user token.
The Muxiva process reads secrets from its environment or a future secret-provider
interface and must redact them from logs, errors, metrics, EventBus payloads,
Graph JSON, Studio state, and recordings.

## Dependency boundary

```text
Muxiva public Node/Frame ABI <- Python Qwen Node Pack
Muxiva public C++ ABI        <- C++ Agora Nodes

Core / Graph / Studio -X-> Qwen, DashScope, or Agora SDK
```

Official and project Node packages may depend one-way on stable Muxiva contracts. The framework
workspace, root build, Registry, and Studio UI may not import, link, register,
or name a vendor. Discovery and configuration flow only through generic Node
Pack Manifests.

## Configuration contract

The current application Node Packs declare these connection fields:

```text
DASHSCOPE_API_KEY
DASHSCOPE_WORKSPACE_ID
MUXIVA_AGORA_APP_ID
MUXIVA_AGORA_CHANNEL
MUXIVA_AGORA_BOT_UID
MUXIVA_AGORA_BOT_TOKEN
```

Qwen model, voice, instructions, and turn mode are non-secret Node
configuration in the application Graph. A future browser-room control service
will mint a separate short-lived user token; it is not part of the current Bot
Node Pack connection.

Temporary console tokens are sufficient for development. A production
deployment must mint short-lived user and Bot tokens from a trusted service;
the App Certificate never reaches the browser or Graph document.

## Interruption contract

When Qwen reports `input_audio_buffer.speech_started`, the owning Node emits
`muxiva.voice.speech.started`. Core routes the opaque Signal only through
explicit Graph Edges. In the Realtime profile, the Qwen Audio Node cancels its
active remote response and rejects late chunks. In Demo 2, Qwen ASR Server VAD
is the Signal source; cancellable background Qwen LLM/TTS workers close their
active HTTP SSE/WebSocket connections, generic gates advance sequence
watermarks, and Agora Audio Sink clears pending PCM. No voice Turn identity or
vendor cancellation logic exists in Core.

An EventBus notification may mirror the result for UI and telemetry, but the
EventBus is not the real-time interruption path.

## Product command and modes

`muxiva studio` is the product entry point. With no argument it discovers the
current project, and from the Muxiva source root it opens the flagship Voice Agent
workspace. `muxiva doctor --voice` reports native-pack and credential readiness
without printing secrets. Missing credentials remain actionable in Studio and
never trigger a silent fallback to scripted output.

The deterministic, network-free path is named `muxiva simulate` and is documented
only as a Runtime engineering fixture. A future local-device transport can
reuse the same Qwen provider without Agora.

## Studio connection and template experience

Studio owns a first-class **Connections** surface. A developer can paste the
DashScope API key and two short-lived Agora RTC tokens, set workspace, App ID,
channel, browser UID, and shared Muxiva Bot UID, and see readiness without manually editing an
environment file. Password fields are cleared immediately after submission. The browser receives
only explicitly `client_exposed` bootstrap fields and never receives DashScope credentials, the
Bot token, or an App Certificate. Local development values persist in the Git-ignored project
`.env` with mode `0600`; production uses a secret store and token service, never Graph JSON.

The Voice Graph Gallery exposes two choices:

1. **Qwen Realtime**: Agora ingress, input resampler, one Qwen Audio Realtime
   Node, captions, output resampler, and Agora Sink. This is the recommended
   lowest-latency product path.
2. **Qwen Full-Duplex Cascade (Demo 2)**: Agora ingress, input resampler, Qwen
   Server VAD + realtime ASR, turn/context fusion, cancellable background Qwen
   streaming LLM, text cancellation gate, cancellable background Qwen realtime
   TTS, captions, output resampler, and Agora Sink. A generic interval Node
   drives short result-drain callbacks without placing vendor scheduling in
   Core. Every intelligence stage uses Alibaba Cloud but remains replaceable.

A template is visible before its optional Provider is installed. It can be
inspected and applied, while validation and Run require every exact Factory
identity in the Runtime Registry. This keeps architecture discovery available
without pretending a missing provider is executable.

## Acceptance

The first live release is accepted only when a new developer can speak in the
browser, see partial/final transcript and graph activity, hear streaming audio,
interrupt it by speaking, and observe the old turn disappear immediately.
Tests must also cover bounded queues, provider reconnect, invalid credentials,
token expiry, late events after cancellation, browser disconnect, and shutdown
during active callbacks. A credential-free test double remains mandatory for
public CI; it must identify itself as simulation rather than a live demo.

## Implementation status

Implemented after the provider-boundary correction:

- Node Pack connection fields stay outside serializable Node configuration and
  are passed only to the owning language process as declared environment values;
- the application-owned Python Qwen Node Pack performs authenticated WebSocket
  setup, session configuration, PCM append, event decoding, and cancellation;
- `qwen.audio_realtime` accepts 16 kHz mono PCM16 and emits 24 kHz
  PCM16 plus incremental text without a Rust provider dependency;
- Agora implementation and Node sources are C++-only under the provider and
  flagship application directories;
- Studio reads connection fields and templates from generic project Manifests
  and contains no provider-specific registry or UI code;
- the built-in PCM16 resampler covers 48 → 16 kHz and 24 → 48 kHz;
- credential-free protocol and fake-transport tests cover authentication,
  audio append, transcript decode, cancellation, bounds, and secret redaction.
- a generic C++ dynamic Node Pack loader validates ABI identity and exact
  Manifest port shape while retaining the loaded library for Node lifetimes;
- Qwen ASR Server VAD emits speech Events, final transcripts, client Events,
  and the explicit interruption Signal;
- Qwen LLM HTTP SSE and Qwen TTS WebSocket I/O run on cancellable background
  workers; 20 ms Runtime ticks drain bounded result queues while leaving
  `on_signal` responsive;
- the Demo 2 Signal fans out to LLM, TTS, text/client gates, and Agora playback;
  late outputs retain the originating audio sequence so every watermark rejects
  the same cancelled response;
- the project Voice Room joins through Agora Web SDK as an independent client;
  it neither controls the Studio Runtime nor reads its EventBus. Audio and
  versioned client events cross the Agora media/data transport and the room
  stays active until the developer ends the session;
- browser-visible connection values require explicit `client_exposed` opt-in;
  DashScope keys, bot tokens, and App Certificates remain unavailable.

Still required for release certification rather than implementation completeness:
retained credentialed live-room evidence on each release platform, token-expiry
and reconnect fault runs against the selected Agora SDK release, and long-duration
soak results. These cannot be fabricated by credential-free CI.
