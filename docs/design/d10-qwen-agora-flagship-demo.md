# D10: Qwen Realtime and Agora flagship voice demo

## Decision

The first live Voxa voice demo uses two optional providers:

- Agora RTC transports user media between a browser and the Voxa Bot.
- Alibaba Cloud Model Studio Qwen Audio Realtime provides acoustic turn
  detection, transcription, reasoning, and streaming speech generation over
  one server-side WebSocket.

The initial low-latency model profile is
`qwen-audio-3.0-realtime-flash` in the Beijing region. Provider model names,
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

Voxa still owns the runtime semantics. The Qwen provider may suggest acoustic
or semantic turn boundaries, but Voxa assigns `TurnId`, converts provider
events into typed Signal/Event Frames, cancels or seals old work, filters stale
output immediately before the Sink, and records latency and queue metrics.

## Media path

```text
browser microphone
  -> Agora Web SDK microphone track
  -> Agora room
  -> Voxa Agora per-user PCM ingress (48 kHz PCM16 mono)
  -> bounded resampler (48 kHz -> 16 kHz)
  -> Qwen Realtime WebSocket
  -> streaming transcript + response audio (24 kHz PCM16 mono)
  -> bounded resampler (24 kHz -> 48 kHz)
  -> Voxa Agora custom audio track
  -> browser remote-track playback
```

No browser request may contain a DashScope API key or Agora App Certificate.
The browser receives only an App ID, channel, UID, and short-lived user token.
The Voxa process reads secrets from its environment or a future secret-provider
interface and must redact them from logs, errors, metrics, EventBus payloads,
Graph JSON, Studio state, and recordings.

## Dependency boundary

```text
Voxa public Node/Frame ABI <- Python Qwen Node Pack
Voxa public C++ ABI        <- C++ Agora Provider and Node Pack

Core / Graph / Studio -X-> Qwen, DashScope, or Agora SDK
```

Provider packages may depend one-way on stable Voxa contracts. The framework
workspace, root build, Registry, and Studio UI may not import, link, register,
or name a vendor. Discovery and configuration flow only through generic Node
Pack Manifests.

## Configuration contract

The current application Node Packs declare these connection fields:

```text
DASHSCOPE_API_KEY
DASHSCOPE_WORKSPACE_ID
VOXA_AGORA_APP_ID
VOXA_AGORA_CHANNEL
VOXA_AGORA_BOT_UID
VOXA_AGORA_BOT_TOKEN
```

Qwen model, voice, instructions, and turn mode are non-secret Node
configuration in the application Graph. A future browser-room control service
will mint a separate short-lived user token; it is not part of the current Bot
Node Pack connection.

Temporary console tokens are sufficient for development. A production
deployment must mint short-lived user and Bot tokens from a trusted service;
the App Certificate never reaches the browser or Graph document.

## Interruption contract

When Qwen reports `input_audio_buffer.speech_started`, the provider emits an
adjacent Signal stamped with the current/new turn boundary. The Turn controller
must then atomically:

1. interrupt the active response;
2. cancel provider generation and stop admitting its old audio;
3. clear Voxa's pending TTS queue and the Agora sender buffer;
4. transition to the new `TurnId`;
5. reject late audio carrying the old turn at the Sink gate.

An EventBus notification may mirror the result for UI and telemetry, but the
EventBus is not the real-time interruption path.

## Product command and modes

The target command is `voxa voice serve`. It starts the Bot and a loopback-only
Voice Playground, opens the browser, performs readiness checks, and never
silently falls back to scripted output. Missing credentials produce an
actionable setup screen.

The old deterministic path becomes `voxa demo --mock`. A future local-device
transport can reuse the same Qwen provider without Agora.

## Studio connection and template experience

Studio owns a first-class **Connections** surface. A developer can paste the
DashScope API key and Agora Bot token, set non-secret workspace, App ID,
channel and Bot UID fields, and see readiness without editing an environment
file. Password fields are cleared immediately after submission. The browser
does not store secrets and the status API never echoes them. Initial storage is
process memory and is erased when Studio exits; durable storage may only use an
OS credential vault, never Graph JSON or browser storage.

The Voice Graph Gallery exposes two choices:

1. **Qwen Realtime**: Agora ingress, input resampler, one Qwen Audio Realtime
   Node, captions, output resampler, and Agora Sink. This is the recommended
   lowest-latency product path.
2. **Qwen Cascade**: Agora ingress, input resampler, local VAD, Qwen realtime
   ASR, turn/context fusion, Qwen streaming text LLM, Qwen realtime TTS,
   captions, output resampler, and Agora Sink. This makes each stage observable
   and replaceable.

A template is visible before its optional Provider is installed, but Studio
must not apply it until every exact Factory identity is present in the Runtime
Registry. This prevents a gallery action from creating a Graph that cannot
validate or run.

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
- `provider.qwen.audio_realtime` accepts 16 kHz mono PCM16 and emits 24 kHz
  PCM16 plus incremental text without a Rust provider dependency;
- Agora implementation and Node sources are C++-only under the provider and
  flagship application directories;
- Studio reads connection fields and templates from generic project Manifests
  and contains no provider-specific registry or UI code;
- the built-in PCM16 resampler covers 48 → 16 kHz and 24 → 48 kHz;
- credential-free protocol and fake-transport tests cover authentication,
  audio append, transcript decode, cancellation, bounds, and secret redaction.

Still required before the credentialed demo meets this record's acceptance:
the generic compiled C++ Node Pack loader, browser room controls, full cascade
Providers, bounded reconnect/mailbox behavior, TurnId stale-output gating, and
retained live-room evidence.
