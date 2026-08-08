# Stage 5 concurrent runtime and flow control

Date: 2026-08-01

Stage 5 adds bounded concurrent graph execution, standalone adaptive realtime
flow-control components, and isolated managed-service sessions to Muxiva's
pre-release foundation. This is an implementation report, not a performance,
transport-readiness, or release-readiness claim. Stage 4's deterministic
`GraphRunner` remains available and unchanged.

## Delivered scope

### 5A: bounded concurrent graph runtime

- `ConcurrentRuntime` compiles an owned `GraphDefinition`, exact
  `NodeInstances`, enabled named `EdgePolicies`, and explicit
  `RuntimeOptions`. `start()` is fallible and returns a `GraphRuntime` only
  after all execution domains and the coordinator have started.
- `GraphRuntime` exposes idempotent `stop`, lifecycle state, EdgeId-keyed
  metric snapshots, bounded `wait`, and abort diagnostics. A bounded wait
  names still-active Nodes instead of joining indefinitely.
- `StopToken`/`Cancellation` provides cross-thread, idempotent cancellation
  and condition-variable wakeup without polling.
- `EdgeQueue` is bounded and holds concrete `Frame`s only. Its `Block`,
  `DropOldest`, `DropNewest`, and `Abort` overflow behavior is explicit;
  `Drain` and `Discard` close modes wake blocked producers, consumers, and
  fair multi-edge fan-in. A later `Discard` monotonically escalates `Drain`.
- Queue metrics record capacity, current and high-water lengths, enqueue,
  dequeue, drop, full, and signal totals, blocked duration, oldest queued
  frame age, and a bounded latest reason. Queue errors and loss reasons are
  retained at the stable EdgeId boundary.
- Every Node has one owned OS worker and one active synchronous callback at a
  time. Preparation is gated; source work starts only after preparation;
  multi-input workers use wake-driven fair fan-in; and Node and EdgePolicy
  callbacks do not execute on the `start` caller thread.
- Routing preserves the declared order: type gate, validation, transform,
  queue, then downstream. Enabled outgoing Edges receive isolated dispatcher
  domains with bounded one-batch mailboxes, so a blocked branch does not
  serially withhold an already-produced batch from a bypass branch.
- Cancellation, errors, panics, normal completion, and lifecycle cleanup use
  a linearized terminal outcome. The first abort reason wins; the coordinator
  joins worker domains before reverse-topological finish or abort hooks.
- A configurable non-zero per-lifecycle-call emission budget bounds retained
  `NodeContext` emissions. Ignoring its structured emission error still
  produces the stable concurrent-runtime abort rather than unbounded growth.
- Partial thread-start failure is structured (`RuntimeStartError`); the launch
  gate prevents user callbacks, started workers are cancelled and joined, and
  startup returns a recoverable error.

### 5B: adaptive realtime building blocks

- Public `RealtimeContract`, `RuntimeInputTuning`, and
  `RealtimeInputProfile` make delivery guarantee, ordering, upstream pause,
  trusted-VAD permission, latency/deadline, and simultaneous frame, byte,
  media-duration, merge, and in-flight bounds explicit and validated.
- `AdaptiveFlowController` is per input port. It records input, admission,
  completion, and drop measurements; tracks fixed-alpha service and rate
  estimates; exposes Normal, Pressure, and Critical states; applies
  three-sample resume hysteresis; and selects only declared overflow actions.
- `AdmissionSlots` and their owned `AdmissionLease`s provide bounded,
  non-polling admission and exact-once release through synchronous or
  asynchronous terminal completion.
- Every admission produces one non-cloneable, controller-bound `FlowWork`
  completion capability. A compatible measurement from another admission or
  controller cannot complete it.
- Audio-prefix merging produces a new immutable audio Frame only for
  compatible, contiguous inputs, preserves ordered bounded lineage and media
  time ranges, and handles both interleaved and planar sample layouts with
  checked cumulative-sample arithmetic. The trusted merge constructor builds
  the ordered interleaved/planar payload itself, so callers cannot attach
  authentic merge lineage to substituted bytes.

### 5C: isolated managed service sessions

- `ManagedAsyncStream` binds one bounded session executor to one `SessionId`.
  It has non-blocking submission and result polling, bounded input,
  completion, strict-order, and result mailboxes, plus a bounded per-session
  in-flight window.
- `ManagedStreamAdapter` translates owned request Frames into result Frames
  behind the isolated executor. Deadlines, cancellation, retry/reconnect
  boundaries, adapter panics, idempotent stop, and late-result discard are
  explicit; carried admission leases release exactly once.
- Per-response Frame and aggregate logical payload-byte limits are configured
  explicitly and checked before result-mailbox insertion. Oversized responses
  become bounded `managed_stream_response_limit` terminal errors and their
  Frames are discarded.
- Cancellation and deadline expiry share one terminal winner. A cancelled
  strict-order request leaves only a tombstone; it cannot later publish a
  timeout or adapter response. Worker completions cannot be stranded in the
  executor's drain-to-sleep transition.
- `ManagedStreamMetricsSnapshot` exposes capacity, lifecycle, retry,
  timeout, cancellation, result-backpressure, ordering, and delivery
  counters. A slow session cannot occupy another session's executor/window or
  block the submitting caller.

## Validation

Fresh validation for the final documentation commit recorded the following.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | passed |
| `cargo test --workspace --all-targets --all-features` | passed: 116 unit/integration tests |
| `cargo test --doc --workspace --all-features` | passed: 6 compile-fail doc tests |
| focused `concurrent_runtime`, `realtime_flow`, and `managed_async_stream` integration tests | passed: 14, 12, and 8 tests respectively |
| `hello`, `frames`, and `text_graph` example binaries | passed; `text_graph` printed `Collected uppercase text: HELLO, MUXIVA` |
| dependency and source audit | passed; only the established `thiserror` and tracing support tree, no Tokio/async runtime, network/protocol, FFI, Python, Serde, or unsafe implementation surface |
| `git diff --check` | passed |

## Known debt and deferred integration

The following limitations are real and intentional; none should be read as
completed MVP behavior.

1. **No full 5B runtime wiring.** `AdaptiveFlowController`, realtime profiles,
   `AdmissionSlots`, and audio merging are standalone components. They are not
   wired into every `ConcurrentRuntime` Edge, NodeRegistry, or Studio path.
   Stage 5C's documented integration order remains: acquire a port slot before
   dequeue, measure bytes/media once for controller events, merge immediately
   before admission, and retain the admission lease and `FlowWork` through the
   managed-service terminal outcome. `AdmissionSlots::close` must still be
   bridged from the graph/session stop path.
2. **Edge metrics remain 5A-shaped.** `EdgeMetricsSnapshot` does not yet carry
   realtime byte totals, media-duration totals, or the new realtime
   drop-reason counters. Those exact measurements currently live in the
   standalone flow snapshot instead.
3. **No registry/serialization/UI exposure.** Resolved realtime profiles and
   flow metrics are not yet exposed through a NodeRegistry, JSON/CLI
   diagnostics, metric-subscription service, or Studio. Public profile fields
   and controller snapshots are inspectable in Rust only.
4. **No Stage 6 signal routing.** Pressure/resume values are bounded
   observations; they are not adjacent `SignalFrame` delivery and there is no
   global EventBus implementation.
5. **Drain is not full-pipeline drain.** Stop closes all Edges to wake waiters;
   therefore Stage 5A only drains Frames already queued per Edge. A transform
   cannot reliably enqueue its drained output into an already-closed downstream
   Edge. Full transform-propagating drain needs the future source/EOS
   admission state machine.
6. **Managed streams are not a network protocol implementation.** The current
   session executor and request workers use standard-library threads and
   closure/service adapters, not Tokio, sockets, transport parsing, or a real
   async reactor. A real transport must preserve the present bounded
   isolation/capacity contract. Managed-stream results do not yet re-enter the
   Stage 5A Edge pipeline; that remains a bounded adapter-integration point.
7. **Blocking user/adaptor code cannot be force-killed.** Runtime waits return
   active-Node diagnostics; stream deadline/cancel/stop becomes logically
   terminal and releases admission, but a synchronous adapter may return only
   later and retains its shared resources until then.
8. **Thread-per-domain is an initial implementation.** Stage 5A uses one OS
   worker per Node plus Edge dispatchers, and Stage 5C uses dedicated session
   and request workers. Later multiplexing may change execution mechanics but
   must retain ownership, bounded admission, and isolation semantics.
9. **Previously recorded review debt remains open where applicable.** The
   following findings from the earlier pre-release reports remain deferred;
   Stage 5 neither silently closes them nor makes a quality-clean claim.

   - Stage 2: default tracing can still emit arbitrary field values; public
     `MuxivaError` builders still cannot attach `Session` or `Stream` context;
     tracing capture/subscriber concurrency, identifier-boundary,
     event-name-wording, stale-example, and literal-versus-summarized-output
     review coverage remains incomplete. License, governance, security,
     release-signing, and publishing decisions also remain deferred.
   - Stage 3: coverage is still missing for equal clock IDs with differing
     kinds and valid padded video strides; `PublicFrameHeaderView` still lacks
     `frame_type`; log-safe byte-length coverage remains incomplete; there are
     no explicit compile-fail guards against future `AsMut`/`DerefMut`; and
     the `frames` example has no exact-output regression test.
   - Stage 4: configuration values are not validated against `ConfigSchema`;
     there is no named-policy registry/factory, serializable graph DTO,
     language binding, or cross-language runtime boundary; `Node::on_abort`
     remains infallible; the synchronous runner stays single-use; its
     `record_delivery` dequeues too early for true queue timing; node-error
     diagnostics discard structured error context; the synchronous design
     still needs normal post-implementation review; and focused coverage still
     lacks an ascending-`EdgeId` fan-out ordering assertion.

## Validation totals

Final totals and audit results are updated in this report only from the fresh
commands run for this documentation commit; no performance measurements are
claimed.
