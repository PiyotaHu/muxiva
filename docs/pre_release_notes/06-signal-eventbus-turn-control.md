# Stage 6 pre-release report: Signal, EventBus, and turn control

Date: 2026-08-01

## Delivered

- `TurnId`, adjacent `SignalFrame` routing, `NodeContext::emit_signal`, and the
  non-lifecycle `Node::on_signal` callback.
- One bounded FIFO Signal queue per enabled edge, cross-thread node delivery,
  EdgePolicy observation, metrics, and coordinated close/Stop handling.
- Global bounded `EventBus` with exact-topic publish, subscription,
  unsubscription, slow-subscriber isolation, handler error/panic metrics, and
  bounded cleanup.
- Graph-level typed `ResourceStore` using `TypeId` and `Arc<dyn Any + Send +
  Sync>` with distinct missing/type errors.
- Atomic `TransportSnapshot`, turn/audio/user/connection/interruption state,
  namespaced Signal/Event schema application, immutable turn stamping, and the
  mandatory stale-turn gate immediately before concurrent Sink callbacks.
- A runnable `control_plane` example, focused integration tests, design
  contract, README links, and this report.

## Public API summary

- `NodeContext::emit_signal`, `Node::on_signal`;
- `RuntimeOptions::with_signal_queue_capacity`;
- `GraphRuntime::{signal_metrics,event_bus,resources,transport_control}`;
- `EventBus::{publish,subscribe,unsubscribe,request_stop,stop}`;
- `ResourceStore::{insert,get,seal,stop}`;
- `TransportControl::{snapshot,apply_signal,apply_event,stamp_frame,
  should_deliver_to_sink}` and idempotent state methods; and
- `TurnId`, `ConnectionState`, `TransportSnapshot`, and structured result/error
  types.

## Verification

The final handoff report records exact command output and test totals. Focused
coverage includes adjacent/no-edge Signal behavior, cross-thread FIFO order,
EventBus subscribe/unsubscribe/slow/error/panic handling, resource type errors,
turn transition/stale filtering/repeated interruption, runtime Sink filtering,
and concurrent stop races.

## Deferred, non-blocking debt

- Per-edge declarative Signal capacity and CLI/Studio visibility.
- Unified metrics export for EventBus and stale-turn counters.
- Synchronous-runner Signal delivery (the Stage 6 requirement specifically
  forbids direct cross-thread delivery; production control routing uses the
  concurrent runtime).
- A hard-stuck EventBus handler remains an unfinished bounded-stop diagnostic;
  safe Rust cannot forcibly terminate arbitrary user code.
- Turn stamping is explicit and untagged frames are accepted for graphs outside
  a turn-managed transport.

No C ABI, Python, TypeScript, Studio, RTC, FFmpeg, or other later-stage surface
was introduced.
