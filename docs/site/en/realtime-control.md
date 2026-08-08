# Real-time flow and control

A real-time agent does more than pass A's output to B. It must decide what happens when a
consumer slows down, how an interruption stops an old answer, and which messages belong to
the business pipeline versus observability.

Muxiva separates the data plane from the control plane:

```mermaid
flowchart LR
    N1["Upstream Node"] -->|"Frame over a typed Edge"| N2["Downstream Node"]
    N1 -.->|"Signal · explicit Graph Edge"| R["Rust Runtime"]
    N1 -.->|"Notification · process-local observation"| B["NotificationBus"]
    R -.->|"on_signal"| N2
    B -.-> UI["Studio · logs · metrics · application"]
```

## Frames carry business data

Audio, video, text, and byte Frames travel over Graph Edges. Port types, queue capacity,
overflow policy, and topology govern their delivery. Receiving a Frame causes the downstream
Node's `on_process` callback to run.

## Signals change runtime state

Signals express interruption, cancellation, cache flushes, and other cross-Node control. A Node
calls `ctx.emit_signal(...)`; the Runtime routes it only to receivers connected by outgoing Graph
Edges and invokes their `on_signal`. Core does not interpret Signal names or execute voice-product
policy. A Signal is not a process-global broadcast.

A common example is barge-in. Qwen Realtime or a VAD Node emits
`muxiva.voice.speech.started`. The Qwen Node cancels its own generation and discards late chunks;
the Agora Audio Sink clears playback when it receives the same Signal. Runtime only delivers it.

## NotificationBus lets observers see what happened

Notifications are process-local observations such as a completed transcript, first-token
arrival, Node reconnection, or excessive latency. A Node calls `ctx.publish_notification(...)`.
Studio, logs, metrics, or application subscribers can observe them, but a NotificationBus
notification does not replace an `EventFrame` or other business data flowing through the Graph.

| Requirement | Use |
| --- | --- |
| Send audio to ASR | Frame + Edge |
| Tell relevant Nodes to stop an old answer | Signal |
| Show local operational telemetry in Studio | NotificationBus notification |
| Deliver transcript or speech state to a remote client | Frame + Transport Node |
| Send LLM text to TTS | Frame + Edge |

## Bounded queues and backpressure

Every Edge queue has a fixed capacity. A full queue follows an explicit policy:

| Policy | Behavior | Typical use |
| --- | --- | --- |
| `block` | Wait for downstream capacity | Text or commands that must stay complete |
| `drop_oldest` | Remove the oldest Frame to stay current | Live audio or video preview |
| `drop_newest` | Preserve already queued data | Stable batch processing |
| `abort` | Fail and begin shutdown | Protocols where loss is unacceptable |

An unlimited queue appears lossless but turns a short slowdown into high latency and unbounded
memory use. Muxiva makes capacity and policy explicit so latency, completeness, and failure
behavior remain predictable.

## Application turns and interruption

If an application needs turns, a model Node, context Node, or project Node owns them—not Core.
On interruption, relevant Nodes normally:

1. cancel the current remote model request;
2. discard late chunks from that request;
3. clear audio that has not played yet;
4. publish local operational state through NotificationBus and client state through a Transport Node; and
5. keep subsequent input flowing through the Graph.

Policy stays in Nodes and mechanism stays in Core. This coordinates model generation, playback,
and observation without coupling the generic Runtime to one model or voice protocol.

## Lifecycle and shutdown

A normal run follows `prepare → process → finish`; errors, timeouts, or cancellation enter
`abort`. The Runtime uses bounded waits for workers and foreign execution domains so a process
cannot report completion while a background thread still owns a microphone, connection, or
model stream.

Next: [Node extensibility](extensibility.md) and the [end-to-end voice path](voice-architecture.md).
