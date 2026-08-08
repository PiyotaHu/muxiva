# Muxiva Stage 6 Signal, EventBus, and Resource Contract

Status: **implemented**

Contract version: **0.1.0-draft.1**

Last updated: **2026-08-02**

## 1. Scope and assumptions

Stage 6 adds two notification scopes and no third message family:

- a `SignalFrame` travels only from a node to the directly connected downstream
  nodes of its enabled graph edges; and
- an `EventFrame` travels through the graph-global `EventBus` to subscribers of
  its exact topic.

Both remain immutable Stage 3 `Frame` variants. Their `NamespacedName`,
`SchemaVersion`, common header timestamp, source `NodeId`, and closed `Value`
payload are authoritative. There is no bare JSON, bare `Value`, media-event, or
implicit parameter channel.

The original Stage 6 implementation also placed voice Turn and interruption
policy in Core. The 2026-08-02 architecture review removed that coupling:
Signal delivery remains a Core mechanism, while Turn, cancellation, stale
response handling, and playback clearing are Node-owned policy.

`on_signal` is a control callback, not a lifecycle callback. The lifecycle is
still exactly `on_prepare`, `on_process`, `on_finish`, and `on_abort`.

## 2. Adjacent Signal routing

`NodeContext::emit_signal(SignalFrame)` validates that the payload source equals
the active node and that at least one enabled downstream edge actually exists.
No edge returns structured `SignalEmissionError::NoConnectedDownstream` and the
stable converted code `MUXIVA-SIGNAL-NO-EDGE`. Signal emissions share the bounded
per-callback emission budget.

The concurrent runtime fans a signal out only to those actual downstream
edges. Each edge has a separate bounded FIFO signal queue (64 by default,
configurable through `RuntimeOptions`). Delivery is:

```text
source node worker -> bounded edge-dispatch mailbox -> EdgePolicy::on_signal
                   -> bounded per-edge Signal queue -> target node worker
                   -> Node::on_signal
```

No source worker directly calls a downstream node. A target callback therefore
runs on the target node execution domain. One source owns its edge-dispatch
sender, so FIFO order is preserved per edge/source. A full signal queue aborts
with an observable structured reason instead of growing without bound or
silently losing control state. `GraphRuntime::signal_metrics` reports capacity,
depth, enqueue/dequeue, and full totals by `EdgeId`.

Signal queues close alongside media queues. Drain preserves already-queued
signals; discard removes them. Queue close wakes the target node worker, and
Stop/error races retain Stage 5 first-error and at-most-once abort semantics.

## 3. Global EventBus

`EventBus` exposes exactly the core operations:

- `publish(EventFrame)`;
- `subscribe(NamespacedName, handler) -> Subscription`; and
- `unsubscribe(Subscription)`.

Every subscriber owns a dedicated OS worker and bounded mailbox. `publish`
uses `try_send`: it never executes a handler and never waits for a slow
subscriber. A full mailbox drops that subscriber's copy only and increments
`dropped_full`; it cannot delay media, Signal delivery, or another subscriber.
Exact topic equality is required.

Each handler invocation is protected by `catch_unwind`. Returned errors and
panics increment separate observable counters and do not terminate publication
or another subscription. `unsubscribe` removes admission immediately and
defers worker reaping. `request_stop` rejects new publish/subscribe operations
and disconnects mailboxes without waiting; `stop(timeout)` performs bounded
cleanup and reports unfinished handlers rather than claiming to kill arbitrary
user code.

The concurrent runtime owns cloneable EventBus and ResourceStore handles. Stop
seals their admission immediately. The coordinator reaps EventBus workers and
releases graph resources only after node workers and reverse lifecycle cleanup.

## 4. Type-safe graph resources

`ResourceStore` maps an explicit bounded `ResourceKey` to an
`Arc<dyn Any + Send + Sync>`, retaining the entry's `TypeId` and Rust type name.
`get<T>` returns an `Arc<T>` only after exact `TypeId` validation. Missing and
wrong-type lookups are distinct `ResourceStoreError` variants. `seal` rejects
new inserts but retains existing values for cleanup; `stop` releases all store
ownership after graph lifecycle cleanup.

Resource values are Rust-internal graph resources. They do not cross a future
C ABI as trait objects or Rust allocation pointers.

## 5. Business control stays in Nodes

Core treats every valid Signal name and payload as opaque. It validates source,
bounds queues, preserves per-edge order, and invokes `Node::on_signal`; it does
not assign `TurnId`, switch conversations, cancel model requests, or filter Sink
output according to a voice-specific rule.

For the flagship voice Graph, `qwen.audio_realtime` owns its remote response
state. On detected speech it cancels the active response, discards late chunks,
and emits `muxiva.voice.speech.started`. `agora.audio_sink` receives that Signal
and clears queued PCM. A cascade may put equivalent policy in VAD, context,
model, and playback Nodes. Other applications can define different Signal
schemas without changing Core.

This keeps mechanism and policy separate: a generic Runtime cannot infer
whether a late Frame is invalid merely from a product-level turn model.

## 6. Thread, memory, and stop model

- Frames and payload buffers remain immutable, owned values; queue crossings
  clone reference-counted ownership only.
- Nodes remain single-owner, single-worker objects. `on_process` and
  `on_signal` cannot overlap on one node.
- Event handlers never run on graph, media, Signal, or publishing threads.
- Event and Signal queues are bounded; no Stage 6 queue is unbounded.
- Runtime Stop is idempotent from any thread, closes media and Signal queues,
  seals global control admission, waits for graph workers, performs reverse
  finish/abort, then releases EventBus and resource ownership.

## 7. Known limitations and Stage 7 input

- Signal capacity is runtime-wide rather than declared independently per edge
  in `GraphDefinition`; per-edge schema exposure belongs with the later JSON
  graph/Registry stage.
- Signal overload currently aborts. No lossy policy is provided because
  silently dropping control transitions would be unsafe.
- EventBus metrics are per subscription snapshots but are not yet connected to
  the unified metrics subscriber/CLI surface.
- A handler that never returns cannot be force-killed. Bounded stop reports its
  subscription as unfinished.
- The synchronous Stage 4 runner does not deliver node-emitted SignalFrames;
  the cross-thread Stage 6 contract is implemented by `ConcurrentRuntime`.
- Turn stamping is explicit at the producing node/adapter boundary. Untagged
  frames are intentionally accepted for graphs that do not use turn control.

Stage 7 may expose `on_signal` and the two Frame variants through versioned C
POD/vtables. It must not expose `ResourceStore`, EventBus handlers, Rust trait
objects, or internal queues across the ABI.
