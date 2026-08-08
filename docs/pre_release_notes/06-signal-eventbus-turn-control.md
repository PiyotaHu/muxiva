# Stage 6 pre-release report: Signal, EventBus, and resources

Date: 2026-08-01
Architecture revision: 2026-08-02

## Delivered

- Adjacent `SignalFrame` routing, `NodeContext::emit_signal`, and the
  non-lifecycle `Node::on_signal` callback.
- One bounded FIFO Signal queue per enabled edge, cross-thread Node delivery,
  EdgePolicy observation, metrics, and coordinated close/Stop handling.
- Global bounded `EventBus` with exact-topic publish, subscription,
  unsubscription, slow-subscriber isolation, handler error/panic metrics, and
  bounded cleanup.
- Graph-level typed `ResourceStore` using `TypeId` and `Arc<dyn Any + Send +
  Sync>` with distinct missing/type errors.
- A runnable `control_plane` example, focused integration tests, and design
  contract.

## Architecture revision

The first Stage 6 implementation also exposed `TurnId` and
`TransportControl`, stamped emitted Frames, interpreted a runtime interruption
name, and filtered stale turns before Sink callbacks. Review found that these
were voice-product policies inside a vendor-neutral Core.

They have been removed. Core now routes opaque Signals without understanding
turns or interruption. Model/context Nodes own request and turn state; playback
Nodes own buffered-output cancellation. In the flagship Graph,
`qwen.audio_realtime` emits `muxiva.voice.speech.started`, cancels its own remote
response, and discards late chunks; `agora.audio_sink` clears queued PCM.

## Current public API summary

- `NodeContext::emit_signal`, `Node::on_signal`;
- `RuntimeOptions::with_signal_queue_capacity`;
- `GraphRuntime::{signal_metrics,event_bus,resources}`;
- `EventBus::{publish,subscribe,unsubscribe,request_stop,stop}`; and
- `ResourceStore::{insert,get,seal,stop}`.

## Verification

Coverage includes adjacent/no-edge Signal behavior, cross-thread FIFO order,
EventBus subscribe/unsubscribe/slow/error/panic handling, resource type errors,
opaque Signal delivery, bounded queues, and concurrent stop races.

## Deferred, non-blocking debt

- Per-edge declarative Signal capacity and CLI/Studio visibility.
- Unified metrics export for EventBus counters.
- Synchronous-runner Signal delivery.
- A hard-stuck EventBus handler remains an unfinished bounded-stop diagnostic;
  safe Rust cannot forcibly terminate arbitrary user code.
