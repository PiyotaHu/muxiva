# Voxa Stage 6 Signal, EventBus, and Turn-Control Contract

Status: **implemented**

Contract version: **0.1.0-draft.1**

Last updated: **2026-08-01**

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

The long-form Stage 6 prompt includes interruption in `TransportControl`; a
secondary transcription omitted that one word while retaining the interruption
requirements and tests. This implementation includes interruption.

`on_signal` is a control callback, not a lifecycle callback. The lifecycle is
still exactly `on_prepare`, `on_process`, `on_finish`, and `on_abort`.

## 2. Adjacent Signal routing

`NodeContext::emit_signal(SignalFrame)` validates that the payload source equals
the active node and that at least one enabled downstream edge actually exists.
No edge returns structured `SignalEmissionError::NoConnectedDownstream` and the
stable converted code `VOXA-SIGNAL-NO-EDGE`. Signal emissions share the bounded
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

## 5. TransportControl and turn filtering

`TurnId` is a strong public identifier. `TransportControl` stores a coherent
`TransportSnapshot` behind one `RwLock`; every write updates the entire state
under one exclusive lock and every snapshot clones it under one shared lock.
The snapshot contains:

- current `TurnId` and revision;
- idempotent interruption and audio-ended flags;
- joined users; and
- connection state.

Turn transition atomically replaces the ID and clears turn-local interruption
and audio-ended state. Repeating an interruption for the current turn returns
`AlreadyApplied`; an old-turn request returns `StaleTurn`. User and connection
updates are likewise idempotent.

`apply_signal` and `apply_event` consume the same namespaced schemas:

- `voxa.transport.turn.changed`;
- `voxa.transport.turn.interrupted`;
- `voxa.transport.audio.ended`;
- `voxa.transport.user.joined` / `.left`; and
- `voxa.transport.connection.changed`.

The methods interpret only the existing `Value::Map` payload. They do not
create another event hierarchy or transport channel.

`stamp_frame` derives a fresh immutable Frame and attaches a private
`voxa.transport.turn` v1 extension plus lineage. Before every Sink
`on_process`, the concurrent runtime compares that extension with one current
snapshot. A mismatched frame is dropped and counted; an untagged frame remains
valid for non-turn-scoped graphs. A malformed turn extension aborts rather than
bypassing the gate.

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
