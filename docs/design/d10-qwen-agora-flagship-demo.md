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

## Configuration contract

The first credentialed acceptance path reads:

```text
DASHSCOPE_API_KEY
DASHSCOPE_WORKSPACE_ID
VOXA_QWEN_REGION=cn-beijing
VOXA_QWEN_MODEL=qwen-audio-3.0-realtime-flash
VOXA_AGORA_APP_ID
VOXA_AGORA_CHANNEL
VOXA_AGORA_USER_TOKEN
VOXA_AGORA_BOT_TOKEN
```

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
DashScope API key and Agora user/Bot tokens, set non-secret workspace, App ID,
channel and region fields, and see readiness without editing an environment
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

Implemented in the first live-provider slice:

- graph-local typed resources are exposed through `NodeContext`, so credentials
  never enter serializable Node configuration;
- the Qwen Audio Realtime Rust Provider performs authenticated WebSocket setup,
  session configuration, PCM append, event decoding, and response cancellation;
- the executable `provider.qwen.audio_realtime` Factory accepts 16 kHz mono
  PCM16 and emits 24 kHz PCM16 plus incremental text;
- the built-in PCM16 resampler covers 48 → 16 kHz and 24 → 48 kHz;
- credential-free protocol and local fake-WebSocket tests cover authentication,
  audio append, transcript decode, cancellation, bounds, and secret redaction.

Still required before the credentialed demo meets this record's acceptance:
Agora Factory wrappers and browser room controls, full cascade Providers,
bounded reconnect/mailbox behavior, TurnId stale-output gating, and retained
live-room evidence.
