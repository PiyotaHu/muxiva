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
    VAD --> FUSION["Context / Policy"]
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
| Official Agora Nodes | C++ Node Pack | RTC ingress/egress and PCM Frame conversion | ASR, LLM, or Graph scheduling |
| Runtime Core | Rust | Types, queues, concurrency, opaque Signal routing, shutdown | Vendor requests, voice turns, or product UI |
| Official Qwen Nodes | Python Node Pack | Realtime or ASR/LLM/TTS streams | RTC channels or Edge queues |
| Developer tools | CLI + Studio | Create, configure, validate, run, observe | Production end-user UI |

## Full duplex and barge-in

Full duplex requires more than opening two sockets. An interruption coordinates several layers:

```mermaid
sequenceDiagram
    participant U as User
    participant T as Agora Nodes
    participant R as Rust Runtime
    participant M as Qwen Node
    participant P as Playback

    M->>R: response audio Frame
    R->>T: send playback audio
    T->>P: play the agent answer
    U->>T: user speaks during playback
    T->>R: new audio Frame
    M->>M: model confirms speech and cancels its response
    M-->>R: voxa.voice.speech.started Signal
    R-->>T: route the opaque Signal
    T->>T: Audio Sink clears queued playback
    R->>M: subsequent audio continues into the same Node
```

Interruption semantics live entirely in Nodes. The Qwen Node cancels the remote response and
discards late chunks; the Agora Audio Sink stops queued playback. Core understands neither voice,
turns, nor a particular Signal name—it only broadcasts the opaque Signal reliably. Custom Nodes
can therefore reuse EventBus without placing application policy in the framework core.

## Credential and deployment boundary

- Short-lived Agora RTC tokens can be exposed to the browser with least privilege; production
  uses a token server.
- Qwen API key and workspace ID remain in server-side Connections or environment variables.
- Graph and Node Manifests can enter source control, but real secrets cannot.
- Studio is for local development; a production web app connects through an explicit application
  service boundary.

Run both Graphs by following [Build a real voice agent from scratch](voice-demo.md).
