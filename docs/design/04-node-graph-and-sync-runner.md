# Voxa Stage 4 Node, Graph, and Synchronous Runner Contract

Status: **Stage 4A graph model implemented; Stage 4B execution contract fixed but not implemented here**

Contract version: **0.1.0-draft.1**

Last updated: **2026-08-01**

## 1. Purpose and authority

This document fixes the Stage 4 Node lifecycle, pure graph protocol, builder
validation, and deterministic synchronous execution semantics. It refines the
Stage 1 lifecycle and graph clauses and uses the Stage 3 immutable `Frame`
contract unchanged. A narrower rule here takes precedence only for Stage 4
surfaces.

Stage 4 is split into two implementation increments:

- **4A**, implemented with this contract, provides lifecycle interfaces,
  descriptors, stable graph data, validation, cycle detection, and stable
  topological order.
- **4B** will provide the single-threaded `GraphRunner`, Edge policy execution,
  lineage attribution, panic translation, and lifecycle-order tests.

The split is an implementation boundary, not permission to leave runner
semantics ambiguous. Sections 8 through 11 are normative for 4B.

## 2. Scope and non-goals

Stage 4A adds dependency-light modules in `voxa-core`:

```text
crates/voxa-core/src/
  node.rs       Node lifecycle, descriptors, ConfigMap, NodeContext, abort data
  edge.rs       Edge descriptors, policy selections, queue data, metrics shape
  graph.rs      GraphDefinition, GraphBuilder, validation, topology
```

It adds no threads, async runtime, network, queue implementation, JSON parser,
Serde, FFI, Python, TypeScript, C++, RTC, FFmpeg, dynamic plugin, or node
registry. It does not execute Node or Edge code. Queue and metrics values are
protocol data reserved for the later runtime.

The Stage 4B synchronous runner remains single-threaded. Stage 5 owns worker
isolation, bounded concurrent queues, backpressure, cancellation tokens,
runtime metric mutation, and safe cross-thread stopping.

## 3. Stable names and data boundary

`NodeId` and `EdgeId` remain distinct Stage 2/3 strong types. Stage 4A adds
strong `PortName`, `NodeTypeName`, `ConfigKey`, `EdgePolicyName`, and
`VisibilityLabel` values. These own validated text and cannot be substituted
for one another by accident. Names are 1 through 255 UTF-8 bytes, have no
leading/trailing whitespace, and contain no ASCII controls.

Every value stored by `GraphDefinition` has equality based on owned data. Node
and Edge collections are sorted by their stable IDs, configuration is sorted
by `ConfigKey`, visibility labels are sorted and deduplicated, and topology
uses `NodeId` as the ready-set tie breaker. Consequently, equivalent builder
input produces the same in-memory order regardless of insertion order.

Stage 4 does not derive Serde. A later JSON DTO may encode this model, but must
preserve the IDs, exact port names and types, descriptor policy names,
configuration values, and stable ordering defined here. No meaning may depend
on an address, closure, trait-object vtable, process-local registration order,
or debug-formatted Rust type name.

## 4. Node lifecycle contract

There is one object-safe `Node` trait and exactly four hooks:

```rust,ignore
pub trait Node {
    fn on_prepare(&mut self, context: &mut NodeContext) -> voxa_types::Result<()>;
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()>;
    fn on_finish(&mut self, context: &mut NodeContext) -> voxa_types::Result<()>;
    fn on_abort(&mut self, reason: &AbortReason, context: &mut NodeContext);
}
```

`on_prepare` and `on_finish` default to success. `on_abort` defaults to a no-op.
`on_process` is mandatory. `AudioFrame`, `VideoFrame`, `TextFrame`,
`ByteFrame`, `SignalFrame`, and `EventFrame` do not receive specialized
callbacks. A normal business failure is a `VoxaError`; panic is reserved for a
bug and caught by the runner boundary.

`NodeKind` is descriptor metadata with exactly `Source`, `Transform`, and
`Sink`:

- a Source declares no input port;
- a Transform may declare input and output ports; and
- a Sink declares no output port.

Every usable descriptor declares the process capability. Lifecycle
capabilities describe whether an implementation performs meaningful work in
the other hooks; they do not add hooks or change runner ordering.

### 4.1 Explicit resolution of Source invocation

`Option<Frame>` exists solely to permit Source invocation without inventing a
non-Frame transport token. In the synchronous runner:

1. after all nodes prepare, each Source is invoked exactly once with
   `on_process(None, context)`;
2. a Source emits zero or more concrete Frames through `NodeContext::emit`;
3. Transform and Sink calls always receive `Some(frame)`; and
4. `None` is generated only inside `GraphRunner`; it is never enqueued,
   cloned, emitted, passed to an Edge policy, stored in `GraphDefinition`, or
   transported on an Edge.

This makes Stage 4 sources finite, one-shot producers. A Source that needs to
produce several initial frames emits all of them in its one call. Repeated or
long-lived source polling belongs to the Stage 5 scheduler and must not be
retrofitted as an end-of-stream sentinel on an Edge.

### 4.2 NodeContext and explicit emissions

`NodeContext` owns only the current `NodeId`, an immutable `ConfigMap`, the
optional input `PortName`, and an ordered emission buffer. It contains no
`GraphRunner`, downstream Node, Edge policy, queue, registry, or mutable graph
reference.

`input_port` is `None` for prepare, finish, abort, and Source invocation. For a
frame delivery it identifies the exact target input port without changing the
uniform `on_process` signature. A Node emits with an explicit
`(output_port, Frame)` pair. The runner validates the output port and exact
Frame type before routing. There is no default port, first-port fallback, port
guessing, or implicit conversion.

## 5. Node descriptor and configuration

`NodeDescriptor` contains only:

- stable `node_id` and registered `node_type`;
- `NodeKind`;
- ordered `PortDescriptor` values;
- a `ConfigSchema` represented by the closed Stage 3 `Value` algebra; and
- `LifecycleCapabilities` data.

Each `PortDescriptor` contains its owning `node_id`, exact `port_name`,
`Input` or `Output` direction, and exactly one `FrameType`. There is no
`AnyFrame`, union, wildcard, subtype relationship, or compatibility table.

`ConfigMap` is an owned, immutable-at-call-boundary, `ConfigKey`-ordered map of
Stage 3 `Value` values. Duplicate construction keys fail instead of replacing
an earlier value. `GraphBuilder::set_config` replaces one node's complete map;
schema validation will later be shared with the Node Registry and JSON
validator rather than improvised in Node implementations.

## 6. Edge descriptor and policy selection

`EdgeDescriptor` stores every Stage 4 prompt field as stable data:

- `edge_id`;
- `from_node_id` and `from_output_port`;
- `to_node_id` and `to_input_port`;
- exact `frame_type`;
- bounded `QueuePolicy` and overflow selection;
- `ValidationPolicy` and validation-failure action;
- `TransformPolicy`;
- `EnabledCondition`; and
- `VisibilityDescriptor` with public/private level and sorted labels.

Stage 4A stores policy registry names, never callbacks. `TypeGateOnly` and
`Identity` are the built-in validation and transform selections. Named policy
implementations will be supplied separately to the runner, just as Node
instances are. `EnabledCondition::ConfigEquals` is declarative data; a builder
does not execute it.

Queue capacity is a non-zero descriptor value. The synchronous runner does not
allocate a queue and does not pretend that this setting supplies Stage 5
backpressure. It preserves the value for the common graph protocol.

## 7. GraphDefinition and GraphBuilder

`GraphDefinition` contains stable `NodeDefinition` data, stable
`EdgeDescriptor` data, and the validated topological order. `NodeDefinition`
is a `NodeDescriptor` plus `ConfigMap`. It contains no `Node` implementation.

In particular, `GraphDefinition` never stores `Arc<dyn Node>`, `Box<dyn Node>`,
an Edge policy trait object, a closure, function pointer, runtime handle, or
callback. `GraphBuilder` likewise starts no thread and invokes no user code.
The later runner receives a separate implementation map:

```rust,ignore
BTreeMap<NodeId, Box<dyn Node>>
```

The runner rejects missing or extra Node instances before lifecycle work. This
separation lets a graph definition be validated, compared, cached, displayed,
and eventually serialized without capturing live resources.

### 7.1 Builder API

- `add_node(NodeDescriptor)` validates descriptor ownership, duplicate ports,
  kind/port shape, process capability, and duplicate Node ID.
- `connect(EdgeDescriptor)` requires both nodes and both explicit ports to
  exist, verifies directions, verifies exact types, and rejects duplicate Edge
  ID.
- `set_config(&NodeId, ConfigMap)` replaces configuration for an existing node.
- `build()` performs cycle detection and stores stable topological order.

Connections must spell out `node_a.audio_out -> node_b.audio_in`. The builder
does not infer ports from Node kinds, port count, Frame type, Node names, or
global state.

### 7.2 Exact type gate and diagnostics

For every Edge, all three values must be identical:

```text
source output FrameType == Edge declared FrameType == target input FrameType
```

An `Audio` output cannot connect to a `Video` input, even if a codec or
converter is registered elsewhere. Such a diagnostic carries `edge_id`, full
source and target endpoints, source/target/Edge types, and recommends an
explicit TransformNode from `Audio` to `Video`. If both ports agree but the
Edge declaration differs, the diagnostic tells the caller to correct the
Edge's exact type.

Structured errors also cover duplicate node/Edge/port, port owner mismatch,
invalid kind/port shape, missing node, missing port, wrong direction, config
for a missing node, and cycle. Error display includes all relevant explicit
endpoint context; callers should match variants rather than parse messages.

### 7.3 Cycle and topological semantics

Cycle detection uses every declared Edge, including an Edge currently marked
disabled, because later configuration must not turn a validated DAG into a
cycle. Kahn's algorithm uses a lexical `NodeId` ready set. Parallel Edges each
contribute an indegree, and each is removed independently. The resulting order
is deterministic across node and Edge insertion order.

An empty builder builds successfully into an empty definition with an empty
topological order. A future synchronous runner prepares, invokes, finishes,
and aborts no nodes and returns success. An Edge-only graph cannot be built
because `connect` requires both nodes.

## 8. Normative synchronous GraphRunner execution (Stage 4B)

Before execution the runner validates that its separate Node instance map has
exactly the IDs in the definition and that all named Edge policies resolve.
It owns those instances for the run. Nodes never receive a runner reference.

The deterministic happy path is:

1. call `on_prepare` in stored topological order;
2. invoke each Source once with `None` in topological order;
3. drain explicit emissions in Node call order;
4. route a matching emission across outgoing Edges in ascending `EdgeId`
   order;
5. call each downstream Node with `Some(frame)` and the exact input port;
6. continue until the in-memory work list is empty; and
7. call `on_finish` once per prepared node in reverse topological order.

When a frame fans out, immutable `Frame::clone` supplies Arc-backed read-only
sharing. Delivery ordering is defined by source order, Node emission order,
and Edge ID. No hash-map iteration may affect observable order.

The runner validates every Node emission before applying an Edge: the named
output must belong to that Node, point outward, and exactly accept the emitted
Frame type. Emitting to a missing/wrong-direction/wrong-type port is a node
error and aborts the graph; it does not search for another port.

## 9. Edge policy execution contract (Stage 4B)

The later `EdgePolicy` interface is runtime behavior resolved outside
`GraphDefinition`. It is called with immutable Frames and a restricted
`EdgeContext` that can read graph identity, the current `EdgeDescriptor`, and
an Edge-local metrics handle. It cannot access `GraphRunner`, a downstream
Node, another Edge, or shared mutable Frame data.

For each delivery the order is fixed:

```text
exact type gate -> validate -> transform -> action -> downstream delivery
```

`validate` runs before `transform`. The default policy validates successfully
and forwards the same Frame. Default validation rejection produces Drop and
records metrics; an explicitly configured abort action stops the graph.

Policy behavior is represented only through:

- `Forward(frame)`;
- `Replace(frame)`;
- `Drop(reason)`;
- `Abort(reason)`; or
- `EmitSignal(signal_frame)`.

The interface may also receive adjacent `SignalFrame` and drop observations,
but Signal is still a concrete Frame. A policy cannot mutate an existing
Frame or buffer. `Replace` must return a distinct derived Frame. The runner
verifies exact type and automatically appends lineage with the parent frame,
current `EdgeId`, and a non-sensitive policy reason. `EmitSignal` accepts only
`FrameType::Signal`; its routing remains adjacent and explicit. Policy panic
or error is translated into `AbortReason` at the protected task boundary.

## 10. Abort and lifecycle guarantees (Stage 4B)

`AbortReason` has stable categories for cancellation, Node-returned error,
Rust panic, foreign-language exception/rejected Promise, and external SDK
error. It carries the failing Node when known, `Prepare`/`Process`/`Finish` or
runtime stage, and owned root context with code, message, and ordered details.
It never carries a borrowed panic payload or foreign exception object.

The first failure wins and stops new processing. The runner tracks prepared
and aborted IDs explicitly. It invokes `on_abort` in reverse topological order
for every prepared Node at most once. A Node whose `on_prepare` returned an
error is not marked prepared; that hook must clean up partially acquired local
resources before returning. `on_abort` cannot fail and a panic in it is caught,
recorded, and does not prevent remaining prepared Nodes from being aborted.

Normal completion calls `on_finish`; failure or cancellation calls
`on_abort`. If prepare, process, or finish panics, the protected boundary
converts it to `AbortCategory::RustPanic`. Foreign bindings must translate
exceptions into their explicit categories before returning to Rust; no panic,
exception, or rejected Promise crosses a language/task boundary.

## 11. Metrics shape

`EdgeMetricsSnapshot` is read-only data keyed by `EdgeId` and includes:

- `queue_capacity`, `queue_len`, and `high_watermark`;
- `enqueue_total`, `dequeue_total`, `drop_total`, and `full_total`;
- accumulated `blocked_duration_ns`;
- optional `oldest_frame_age_ns`; and
- optional latest non-sensitive error reason.

Stage 4A supplies only a zero snapshot constructor and accessors. Stage 4B may
update synchronous counters but provides no queue or subscription. Stage 5
owns atomic/thread-safe collection, `snapshot_edge_metrics(edge_id)`, and the
subscription interface. Duration fields use integer nanoseconds in graph and
snapshot data so their future JSON meaning is independent of Rust's in-memory
`Duration` representation.

## 12. Threading, ownership, and privacy

Stage 4A is pure construction and validation on the caller's thread. It
introduces no synchronization primitive or interior mutability. Descriptors,
configuration, and Graph definitions own their data. Live Frames retain the
Stage 3 immutable Arc-backed buffer and derivation rules.

The synchronous runner executes all lifecycle and policy calls serially on its
calling thread. It does not claim callback-thread safety and must never be
called directly from an RTC/SDK callback. Stage 5 will isolate work in runtime
workers. Nothing in Stage 4 changes the fixed Adapter copy/retain/release or
safe-stop rules.

Configuration, abort details, validation reasons, visibility labels, and
metrics reasons must not contain media payloads, credentials, private
extension values, or complete transcripts. `Private` visibility suppresses
default presentation; as in Stage 3, it is not access control or encryption.

## 13. Verification and deferred work

Stage 4A tests cover duplicate Node and Edge IDs, missing ports, wrong
directions, exact Audio/Video mismatch and suggested TransformNode, cycles,
stable topology across insertion order, empty graph, and preservation of
stable Node/Edge/port/config identities.

Stage 4B must add focused tests for lifecycle order, source `None` isolation,
explicit input/output ports, Edge policy order, Replace lineage,
Drop/Abort/EmitSignal, prepare/process/finish failure, Rust panic conversion,
first-error behavior, abort-at-most-once, and resource release. Those tests
must run before Stage 5 introduces concurrency.
