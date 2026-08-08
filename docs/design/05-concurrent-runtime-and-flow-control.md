# Muxiva Stage 5 Concurrent Runtime and Flow-Control Contract

Status: **Stage 5A runtime and standalone Stage 5B/5C components implemented**

Contract version: **0.1.0-draft.1**

Last updated: **2026-08-01**

## 1. Purpose and authority

This document upgrades the deterministic Stage 4 graph into a bounded,
observable streaming runtime. It fixes the full Stage 5 behavior for Edge
queues, node execution domains, admission, cancellation, realtime input,
adaptive flow control, audio coalescing, and managed asynchronous services.
Stage 4's pure `GraphDefinition`, immutable `Frame`, exact ports, Node hooks,
and Edge policy order remain authoritative.

Implementation remains intentionally layered:

- **5A**, implemented now, supplies the std-only bounded Edge queue,
  cancellation primitive, per-node synchronous worker domains, admission of
  one, source workers, policy routing, Edge metrics, safe Stop, and bounded
  shutdown observation.
- **5B**, implemented as standalone per-port components, supplies the
  declarative `RealtimeContract`, internal input profile, media-aware limits,
  audio coalescing, pressure prediction, and adaptive controller.
- **5C**, implemented as an isolated standard-library session executor,
  supplies `ManagedAsyncStream`, per-session mailboxes, and in-flight windows.
  Binding these components into every Stage 5A Edge remains staged debt.

Signal delivery and a global EventBus are Stage 6. Stage 5 may produce only
adjacent flow-pressure observations in the future; it does not introduce a
global control bus.

## 2. Invariants

1. Every Edge queue is bounded and stores only concrete `Frame` values.
   End-of-stream, cancellation, work permits, batches, errors, and callbacks
   are queue state or control-plane data, never queue elements.
2. The routing order is `type gate -> validate -> transform -> queue ->
   downstream`. Every observation is attributable to the stable `EdgeId`.
3. A Node instance is owned by one execution domain. Its lifecycle and process
   hooks never overlap. Stage 5A fixes `max_in_flight = 1`.
4. No user Node or EdgePolicy callback runs on the thread calling `start`.
5. Frames and Arc-backed payloads remain immutable. A transform uses `Replace`
   with a fresh frame and automatic lineage; it cannot mutate a fan-out frame.
6. Cancellation and Stop are idempotent and callable from any thread. Closing
   a queue wakes every blocked producer and consumer.
7. The first terminal error wins. Later queue-close errors, panics, and cleanup
   diagnostics cannot replace it.
8. Shared runtime resources outlive processing workers and lifecycle cleanup.
   They are released last.
9. There is no busy wait, unbounded queue, or silent unbounded join.
10. Loss, latency growth, replacement, queue-full events, and abort decisions
    are observable and never described as lossless behavior.

## 3. Stage 5A public runtime

`ConcurrentRuntime::new` receives an owned `GraphDefinition`, the same
`NodeInstances` and `EdgePolicies` maps used by `GraphRunner`, and explicit
`RuntimeOptions`. Construction validates missing and extra implementations,
enabled conditions, and named policy resolution without invoking user code.
`ConcurrentRuntime::start` transfers all live objects into worker domains and
returns a cloneable `GraphRuntime` control handle.

The Stage 4 `GraphRunner` remains available and unchanged for deterministic,
single-threaded execution. The concurrent runtime does not wrap it or alter
its ordering and metrics.

`GraphRuntime` provides:

- idempotent `stop()` (the runtime retains its internal `StopToken` for worker
  and future managed-stream cancellation);
- lifecycle state snapshots;
- coherent `EdgeMetricsSnapshot` values by `EdgeId`;
- `wait(timeout)`, which returns completion, the first `AbortReason`, or
  bounded diagnostics containing the state and IDs of still-active workers;
  and
- bounded abort-hook panic diagnostics.

Dropping a handle does not synchronously join workers. Applications must call
`stop` and bounded `wait` when they own shutdown. The coordinator is retained
by worker/shared state until cleanup completes.

## 4. Bounded Edge queues

`EdgeQueue` is a cloneable handle over a Mutex, two condition variables, and a
bounded `VecDeque<Frame>`. A parallel timestamp deque is queue metadata; it is
not a transported value. Capacity comes from `QueuePolicy` and cannot be zero.

Producer behavior when full is explicit:

- `Block` waits on `not_full`. It records full observations and accumulated
  blocked nanoseconds. A dequeue, discard, or close wakes it.
- `DropOldest` removes exactly the oldest queued Frame, records the stable
  reason, and enqueues the arriving Frame.
- `DropNewest` retains queued Frames and records that the arriving Frame was
  dropped.
- the already-declared Stage 4 `Abort` selection produces a queue-overflow
  error that the runtime converts into the graph's first abort.

Consumers block on `not_empty`. Close has two explicit forms:

- `Drain` rejects producers but retains queued frames until received; and
- `Discard` rejects producers, drops queued frames with
  `ShutdownDiscard`, and makes consumers observe closed immediately.

Both modes notify all producer and consumer waiters plus the target node's
fan-in wake primitive. A node with several incoming Edges scans them fairly
from a rotating cursor and sleeps on this shared notification. It does not
poll.

Stage 5A drain is **per already-enqueued Edge**. Global Stop closes every Edge
immediately to guarantee wakeup, so a Transform draining one input cannot
enqueue newly produced output into an already-closed downstream Edge. Full
pipeline propagation drain requires the 5B source/EOS admission state machine;
this limitation is observable rather than silently pretending to drain.

## 5. Metrics and reasons

Each queue supplies a coherent `EdgeMetricsSnapshot` containing capacity,
current length, high watermark, enqueue/dequeue/drop/full/signal totals,
blocked duration, oldest queued-frame age, and a bounded sanitized latest
reason. Policy rejection, explicit Drop, Abort, invalid replacement, type
failure, queue overflow, and shutdown discard all update that Edge's metrics
before control moves elsewhere.

Queue-policy outcomes use a closed `QueueDropReason` in 5A. Later realtime
reasons must distinguish at least deadline expiry, media-duration overflow,
byte overflow, silence-first trusted-VAD removal, pressure shedding, and
shutdown discard. Audio loss metrics additionally count dropped media
duration; frame count alone is insufficient.

Metrics and reasons must not contain media payloads, complete transcripts,
credentials, private extension values, or panic payload objects. Text is
sanitized and bounded to 256 bytes.

## 6. Worker and admission model

Every Node owns one OS worker in 5A. Workers prepare concurrently and meet at
a preparation barrier. Sources cannot produce until every Node has completed
prepare successfully. Each Source is then invoked once with `None` in its own
worker, preserving the Stage 4 source interface while allowing several Sources
to run concurrently.

Transform and Sink workers take a Frame only when their single admission slot
is free. The slot is held for the complete synchronous `on_process` call and
released when it returns. Therefore a slow Node backpressures only its own
incoming Edge queues and upstream paths; it cannot accumulate an unbounded
mailbox or occupy another Node's execution domain.

Outgoing Edge policies are owned by the upstream Node worker. An Edge has one
declared producer, so its policy is neither shared nor held behind a
continuously contended lock. Each call is inside a panic boundary. Returned
errors and panics become an `AbortReason`. 5B may add an explicit callback
deadline, but cannot abandon a Rust stack or claim to interrupt arbitrary
blocking user code.

Workers close their outgoing Edges with Drain after normal input exhaustion.
This propagates finite end-of-stream through a DAG without putting a sentinel
in a queue.

## 7. Stop and lifecycle order

The safe-stop sequence is normative:

1. state becomes `Stopping` and new Source work is rejected;
2. the first owned cancellation reason is installed and `StopToken` broadcasts;
3. every Edge closes using the declared Stop or failure drain mode, waking all
   waiters;
4. Sources cease production; blocked Edge submissions return closed;
5. processing workers exit and transfer their Node and EdgePolicy ownership to
   the coordinator;
6. the coordinator confirms the worker threads have exited;
7. on normal completion it calls `on_finish` in reverse topological order;
8. on error or cancellation it calls `on_abort` at most once on every prepared
   Node in reverse topological order;
9. abort-hook panics are bounded diagnostics and cleanup continues; and
10. Node instances, Edge policies, queues, metrics, and shared graph resources
    are released only after lifecycle cleanup.

`wait(timeout)` is the bounded observation mechanism. If user code does not
return, the runtime cannot safely kill its thread; the timeout reports the
still-active Node IDs. This is an explicit diagnostic, not a silent join or a
busy-wait workaround.

## 8. Stage 5B realtime contract

Business graph data will declare `RealtimeContract`; it will not ask each Node
author to invent locks, throughput predictions, or queue sizes. The contract
contains stable, JSON-visible fields:

- `latency_budget`;
- `delivery_guarantee`: `Lossless` or `BestEffort`;
- whether Audio Frames are permitted and their acceptable duration range;
- ordering requirement;
- whether upstream is pausable; and
- whether trusted VAD permits silence-first removal.

The runtime derives a `RealtimeInputProfile` for each input port. Internal
tuning includes `max_frames`, `max_bytes`, `max_buffered_media_duration`,
`target_batch_duration`, `max_in_flight`, `deadline`, and `overflow_policy`.
Conservative defaults and derived values must be visible in Registry/Studio,
JSON validation, CLI diagnostics, and metrics. Audio admission enforces frame,
byte, and media-duration ceilings simultaneously.

Overflow actions are closed and explicit:

- `PropagateBackpressure`;
- `DropOldest`;
- `DropNewest`;
- `DropSilenceFirst`, only with trusted VAD evidence; and
- `AbortSession`.

An unpausable microphone/RTC source must declare a terminal action. A
`Lossless` contract can propagate pressure or abort when latency remains above
budget; it cannot silently discard audio. A service that is persistently
slower than realtime cannot be advertised as non-blocking and lossless.

## 9. Adaptive flow controller

The runtime root owns one `AdaptiveFlowController`, with independent state for
each input port and session. It continuously measures admitted media duration,
completion duration, queue age, and service-time EMA. Prediction must enter
Pressure/Critical before a configured hard queue limit when the measured
service rate implies overflow.

Actions are restricted by `RealtimeContract` and prioritized:

1. Normal preserves small batches and declared ordering.
2. Pressure may increase audio coalescing up to the declared maximum, restrict
   new admission, and produce adjacent `flow_pressure` / `flow_resume`
   observations for a pausable upstream.
3. Critical applies only the predeclared overflow action.

Every transition, prediction input, selected action, and result is metricized.
The controller cannot change audio format, reorder ordered traffic, exceed the
declared coalescing duration, implicitly shed lossless audio, or conceal
permanent overload under the word adaptive.

Stage 5B exposes the controller, admission slots, and audio-prefix merger as
standalone per-input-port components. The Stage 5C runtime hook is deliberately
narrow: acquire a port slot before dequeue, record enqueue/admission with the
same byte/media measurement, optionally merge compatible audio immediately
before admission, and retain both the admission lease and controller work
record until the managed request reaches a terminal completion. Flow-pressure
and resume values remain bounded observations until Stage 6; they are not
delivered by calling Nodes directly.

## 10. Audio coalescing

Only consecutive Audio Frames with compatible format, sample rate, channel
layout, clock domain, stream, and contiguous timing may coalesce. The result is
a new `AudioFrame`, never a raw batch type. It spans the exact original media
time, has a fresh frame ID, and records all input frame IDs/time bounds in
bounded lineage. A typical controller may grow 20 ms inputs toward 80 or 100
ms only within the contract.

Coalescing is not loss. Queue byte/media-duration limits apply to the combined
payload, timestamp math is checked, and tests must verify duration, timestamp,
sequence/ordering, payload order, and lineage.

## 11. Stage 5C ManagedAsyncStream

`ManagedAsyncStream` is the sole Stage 5 abstraction for long-lived ASR, TTS,
and model-gateway I/O. It owns connection state, asynchronous writes and
reads, in-flight requests, deadlines, retry boundaries, cancellation, and
result callbacks. Adapters translate responses into concrete Frames and submit
them through runtime admission; ordinary Nodes do not implement Futures,
callbacks, locks, or private queues.

Network send, receive, reconnect, and protocol parsing execute in an isolated
Rust async I/O executor, separate from graph workers and Python execution
domains. A capture receiver may only wrap, coalesce, and perform non-blocking
admission; it never waits for a network response.

Every slow service/session owns an independent input mailbox, send queue,
in-flight window, and result mailbox. Congestion, reconnect, or timeout in one
ASR session cannot block a bypass Edge, another Node, or another session.
Results re-enter the scheduler through a bounded mailbox and preserve the
declared response ordering. Runtime graph threads never block on network I/O.

An async dependency is justified only in 5C at this isolation boundary. The
core queue and scheduler remain Rust standard-library based.

The initial Stage 5C implementation uses a standard-library session executor
instead of adding an async dependency before a real transport requires one.
Each `ManagedAsyncStream` is bound to one `SessionId` and owns bounded input,
completion, and result mailboxes plus a bounded in-flight window. The session
executor launches only window-admitted request workers; adapter connect/send/
parse work therefore never runs on a caller or graph thread. A strict stream
retains out-of-order completions inside that same window until earlier request
sequences resolve. Submission and graph-facing result polling are `try` paths.

Cancellation or deadline expiry releases the carried admission lease exactly
once. A blocking adapter may finish later, but its response is counted and
discarded. Stop rejects new submissions, cancels all registered requests,
clears bounded mailboxes, and lets any detached adapter call retain its shared
resources until it exits. A future real socket transport may replace these
dedicated workers with an async reactor without changing these isolation and
capacity boundaries.

Each adapter response is checked against configured non-zero Frame-count and
aggregate logical payload-byte limits before it can enter a result mailbox.
An oversized response becomes the structured
`managed_stream_response_limit` terminal error; its Frames are dropped. A
cancelled request occupies only an ordering tombstone, and deadline delivery
is permitted only to the thread that wins the request's exact-once terminal
transition.

## 12. Verification and staged debt

Stage 5A tests cover producer/consumer close wakeups, Block, DropOldest,
DropNewest, slow-Sink backpressure, Stop wake and idempotency, first-error
shutdown, concurrent Sources, 10,000-frame zero-loss delivery, Edge metrics,
and proof that Node callbacks do not run on the starting caller thread. The
complete Stage 4 suite remains required.

Stage 5B/5C tests cover 20 ms audio coalescing and lineage, exact cumulative
sample-boundary duration math, byte/frame/media-duration limits,
Pressure/Critical prediction before full, every realtime overflow action,
lossless no-silent-drop, pausable and unpausable inputs, slow simulated ASR
without bypass blockage, independent session in-flight windows, strict result
ordering, terminal cancellation/deadline races, retry boundaries, response
limits, and network isolation.

Recorded nonblocking 5A debt:

- OS-thread-per-node is the initial execution-domain implementation; later
  executors may multiplex domains without weakening ownership/admission.
- Stop Drain is per-Edge buffered drain, not full transform-propagating drain.
- source invocation remains finite and one-shot until the 5B admission/source
  state machine is introduced.
- callback deadlines are diagnostic/cooperative; arbitrary synchronous Rust
  user code cannot be forcibly interrupted safely.
- metric subscription is deferred; snapshots are coherent and queryable now.
- full runtime wiring for realtime contracts, audio coalescing/controller, and
  managed async results remains open, as do JSON/CLI/Studio exposure and
  audio-duration loss metrics.

These are bounded follow-on items. Unbounded memory, silent drops, caller-thread
user code, busy wait, queue wakeup failure, unsafe mutation, and compile/safety
failures are not acceptable debt.
