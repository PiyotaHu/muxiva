# End-to-end voice path

A real voice path demonstrates Voxa's layers clearly. The browser owns user experience, Agora
owns network transport, Qwen owns algorithms, and the Rust Core owns real-time scheduling. No
layer needs the internal implementation of another.

## Two Graph choices

### Realtime model Graph

A realtime model combines speech understanding and generation in one streaming model session,
which shortens the path and supports natural interaction:

```mermaid
flowchart LR
    B["Browser microphone"] --> AI["Agora Audio Ingress<br/>C++ · Transport"]
    AI --> QR["Qwen Audio Realtime<br/>Python · Algorithm"]
    QR --> AO["Agora Audio Egress<br/>C++ · Transport"]
    AO --> S["Browser speaker"]
    QR -."transcript Events".-> UI["Studio / Voice Room"]
```

Choose it when low latency, natural turns, and fewer components are the priority.

### Cascade Graph

A cascade separates capabilities so each model can be selected independently and intermediate
results or business logic can be inserted:

```mermaid
flowchart LR
    IN["Agora Ingress"] --> VAD["VAD"]
    IN --> ASR["Qwen ASR"]
    VAD --> FUSION["Turn / Context"]
    ASR --> FUSION
    FUSION --> LLM["Qwen LLM"]
    LLM --> TEXT["Transcript / Tool"]
    LLM --> TTS["Qwen TTS"]
    TTS --> OUT["Agora Egress"]
```

Choose it when the application needs custom VAD, prompts, tools, moderation, transcripts, or
TTS. Branching and joining are normal Graph capabilities; Voxa is not limited to a linear
`A → B → C` pipeline.

## What each layer owns

| Layer | Implementation | Owns | Does not own |
| --- | --- | --- | --- |
| Project web | HTML/JS + Agora Web SDK | Microphone permission, channel, playback, interaction UI | Model secrets or Runtime scheduling |
| Transport Provider | Agora C++ Node Pack | RTC ingress/egress and PCM Frame conversion | ASR, LLM, or Graph scheduling |
| Runtime Core | Rust | Types, queues, concurrency, turns, interruption, shutdown | Vendor requests or product UI |
| Algorithm Provider | Qwen Python Node Pack | Realtime or ASR/LLM/TTS streams | RTC channels or Edge queues |
| Developer tools | CLI + Studio | Create, configure, validate, run, observe | Production end-user UI |

## Full duplex and barge-in

Full duplex requires more than opening two sockets. An interruption coordinates several layers:

```mermaid
sequenceDiagram
    participant U as User
    participant T as Agora Transport
    participant R as Rust Runtime
    participant M as Qwen Provider
    participant P as Playback

    M->>R: audio Frame for the current turn
    R->>T: send playback audio
    T->>P: play the agent answer
    U->>T: user speaks during playback
    T->>R: new audio Frame
    R->>R: VAD/model confirms barge-in and changes turn
    R-->>M: interrupt / cancel Signal
    R-->>T: stop old-turn audio
    R->>M: begin the new turn
```

The Provider attempts to cancel remote generation while the Runtime filters late results by turn
identity. Even if vendor cancellation is delayed by the network, stale audio cannot re-enter the
current playback path.

## Credential and deployment boundary

- Short-lived Agora RTC tokens can be exposed to the browser with least privilege; production
  uses a token server.
- Qwen API key and workspace ID remain in server-side Connections or environment variables.
- Graph and Node Manifests can enter source control, but real secrets cannot.
- Studio is for local development; a production web app connects through an explicit application
  service boundary.

Run both Graphs by following [Build a real voice agent from scratch](voice-demo.md).
