# Muxiva v0.1 Product and Technical Contract

Status: **Accepted design; Stage 1 foundation contract**

Contract version: **0.1.0-draft.1**

Last updated: **2026-07-31**

## 1. Purpose

Muxiva is a real-time multimodal agent runtime with a single Rust core and a
common node model for Rust, C++, Python, and TypeScript. Its first end-to-end
validation target is a real-time voice-agent pipeline, while its public core
contracts remain media- and vendor-neutral.

This document is normative for v0.1 development. Later stages may refine a
contract only through an explicit design change recorded in pre-release notes.
They must not silently create parallel data paths, lifecycle hooks, ownership
rules, or graph formats.

## 2. Scope

### 2.1 v0.1 MVP

The MVP provides:

1. A static directed acyclic graph composed of `SourceNode`, `TransformNode`,
   and `SinkNode` instances.
2. A programmatic `GraphBuilder` and a stable, serializable JSON
   `GraphDefinition` used by all graph-authoring surfaces.
3. A CLI that validates and runs graphs and starts a local web Studio for
   drag-and-drop editing and validation.
4. A single `Frame` family for all node-to-node information, with audio,
   video, text, byte, signal, and event variants.
5. Multithreaded data flow, bounded queues, basic and adaptive backpressure,
   cancellation, deterministic shutdown, error propagation, and observability.
6. Adjacent-node signals and a process-local notification bus with deliberately separate
   routing semantics.
7. Node development surfaces for C++, Python, and TypeScript that preserve the
   Rust lifecycle and frame semantics.
8. Replaceable C++ adapters for native SDK integration through a stable C ABI.
9. A mock transport voice-agent reference graph that exercises audio-to-text,
   text-to-text, and text-to-audio transforms without requiring external SDKs.

### 2.2 First vertical validation

The first reference graph is:

```text
MockAudioSource.audio
  -> MockAsr.audio -> MockAsr.text
  -> MockLlm.prompt -> MockLlm.response
  -> MockTts.text -> MockTts.audio
  -> AudioSink.audio
```

The reference names describe node behavior, not core concepts. The core must
not contain special ASR, LLM, TTS, conversation-history, or prompt APIs. This
graph validates typed transforms, backpressure, pressure signals, turn
interruption, stale-frame filtering, cancellation, draining, shutdown, and
edge-level metrics.

### 2.3 Explicit non-goals

v0.1 does not implement:

- real RTC or FFmpeg integrations;
- dynamic plugin discovery or hot loading;
- Java bindings;
- GPU scheduling or GPU buffer contracts;
- distributed graph execution or remote scheduling; or
- unrestricted Python async execution.

Python async is limited to controlled I/O. CPU-intensive Python work must use
an explicit worker or native implementation and must not masquerade as async.

## 3. Terminology

| Term | Normative meaning |
| --- | --- |
| Node | A processing component with declared ports and one unified lifecycle. |
| SourceNode | A node that originates frames and has no required data input. |
| TransformNode | A node that consumes frames and produces zero or more frames. |
| SinkNode | A node that consumes frames and has no required data output. |
| Stream | An ordered logical sequence identified by `stream_id`. |
| Frame | The only unit that may carry information between nodes. |
| Graph | A validated static DAG of nodes and explicit edges. |
| Stage | A project-delivery phase. It does not mean a node or graph vertex. |
| Port | A named, directed, exactly typed node endpoint. |
| Edge | A declared connection between one output port and one input port. |
| Signal | A `SignalFrame` routed only across actual adjacent graph edges. |
| Event | An `EventFrame` carried through an explicit typed Graph port and Edge. |
| Notification | A process-local observation published through `NotificationBus` without a Graph port. |
| Adapter | Native integration code that converts an external SDK contract to the Muxiva C ABI. |
| Studio | The local web graph editor that reads and writes `GraphDefinition`. |

Documentation and APIs must use these terms. In particular, component,
processor, pipeline step, and handler are not synonyms for Node in public APIs.

## 4. System boundaries

```text
Python Node ---- PyO3 -----+
TypeScript Node - N-API ---+--> Rust Muxiva Core
C++ Node ------- C ABI ----+    graph, scheduling, queues, backpressure,
C++ SDK -> C++ Adapter ----+    lifecycle, Signal, NotificationBus, stop, metrics
```

### 4.1 Rust Muxiva Core

The core exclusively owns graph validation and execution, scheduling, queues,
admission and backpressure, cancellation, lifecycle coordination, Signal
routing, NotificationBus operation, graph resources, shutdown, and metrics. It must
not depend on a real RTC, FFmpeg, Python, Node.js, or proprietary SDK.

### 4.2 C++ nodes

C++ nodes implement the common Node contract through the versioned C ABI. They
do not receive Rust traits or own core scheduling. C++ exceptions are caught
inside the C++ boundary and translated to stable errors.

### 4.3 C++ adapters

Adapters encapsulate RTC, FFmpeg, codecs, or other native SDKs. They translate
SDK buffers, callbacks, lifetime rules, and errors into the Muxiva C ABI. An
adapter cannot invoke graph logic from an SDK callback thread.

### 4.4 Python nodes

Python nodes bind through PyO3. Each node has an isolated Python execution
domain and controlled I/O event-loop boundary. Python nodes never execute on
an SDK callback thread. Python exceptions become `AbortReason` values.

### 4.5 TypeScript nodes

TypeScript nodes bind through Node-API and execute in a dedicated Node.js
environment, isolated from graph scheduling and SDK callback threads. Thrown
errors and rejected promises become `AbortReason` values.

## 5. Graph contract and authoring surfaces

`GraphDefinition` is the single pure-data representation of a graph:

```text
GraphBuilder ----+
JSON document ---+--> GraphDefinition --> Registry validation --> Runtime
CLI -------------+
Web Studio ------+
```

`GraphBuilder` exposes at least `add_node`, `connect`, `set_config`, and
`build`. It constructs and validates data; it starts no thread, executes no
node, and owns no user callback.

Every node definition contains a stable node ID, registered node type,
configuration, and declared ports. Every edge contains a stable edge ID,
source node and output port, destination node and input port, and exact frame
type. Connections are never inferred from node names or global state. A type
conversion requires a TransformNode; an edge cannot convert implicitly.

The JSON root contains `schema_version`, graph identity, nodes, edges, and
graph-level configuration. IDs and meanings cannot depend on memory addresses,
closures, or process-local registration order. Unknown optional fields are
preserved when a document is round-tripped. Unsupported required semantics
cause a structured validation error.

The Node Registry maps a stable `node_type` and version to port descriptors,
configuration JSON Schema, lifecycle capabilities, implementation language,
and visible realtime defaults. GraphBuilder, CLI, Studio, and Runtime all
validate against this registry rather than maintaining separate rules.

The CLI will provide graph validation, graph execution, and local Studio
startup. The Studio persists the same JSON protocol and does not introduce a
second graph format or browser-only semantics.

## 6. Frame contract

### 6.1 Common header

Every frame has an immutable header containing:

- `frame_id`: globally unique frame identity;
- `timestamp`: signed nanoseconds in a declared clock domain;
- `sequence_id`: monotonically increasing within a stream;
- `stream_id`: stable logical stream identity;
- `trace_id`: distributed diagnostic correlation identity;
- `frame_type`: exact frame variant;
- immutable `metadata`;
- versioned `extensions`; and
- non-payload `lineage` describing parent frames and transformations.

Timestamp values alone are not comparable across clock domains. A graph input
must declare whether its clock is monotonic, media-relative, or wall-clock
derived. Runtime scheduling uses monotonic time; wall-clock time is diagnostic.

### 6.2 Variants

- `AudioFrame`: PCM payload, sample rate, channels, sample format, and duration.
- `VideoFrame`: YUV420P or RGBA payload, width, height, plane/stride data, and
  pixel format.
- `TextFrame`: validated UTF-8 payload.
- `ByteFrame`: opaque binary payload with an optional media type.
- `SignalFrame`: adjacent-control name, schema version, source, timestamp, and
  cross-language `Value` payload.
- `EventFrame`: global topic, schema version, source, timestamp, and
  cross-language `Value` payload.

Signal and Event frames do not create an untyped side channel. Media payloads
must not be sent through NotificationBus. `Value` is restricted to null, boolean,
number, string, bytes, list, and string-keyed map.

### 6.3 Immutability and extensions

Headers and buffers are immutable after construction. A change creates a new
frame and records parent frame IDs, transforming Node/Edge identity, and a
non-sensitive reason in lineage.

Extension keys use a reverse-domain or team namespace. Each extension declares
`key`, `schema_version`, `producer`, `visibility`, and a `Value` or byte value.
Unknown extensions pass through unchanged. Private visibility suppresses
default logs, serialization, and Studio display; it is not encryption or
access control.

## 7. Node lifecycle and error model

The only lifecycle hooks are:

1. `on_prepare`
2. `on_process`
3. `on_finish`
4. `on_abort`

`on_process(frame, context)` is the sole data-processing entry for every frame
type. Public APIs must not add `on_audio_frame`, `on_video_frame`, or similar
type-specific lifecycle hooks.

Prepare runs in topological order. Normal finish runs in reverse topological
order. A node error, cancellation, Rust panic, foreign-language exception,
rejected Promise, or external SDK failure stops the graph. Every prepared node
then receives at most one reverse-topological `on_abort` call.

An error contains a stable category and code, message, Session/Node/Edge when
available, lifecycle or runtime phase, source-language classification, and a
causal context chain. Panics and exceptions cannot cross FFI or task
boundaries. They are caught and translated into `AbortReason`. Business errors
must not be represented by panics.

## 8. Ownership and FFI contract

Cross-language public interfaces permit only versioned C-compatible POD
structures, pointers with explicit lengths, stable error codes, version and
capability values, function pointers with documented calling rules, and opaque
handles. They must not expose C++ classes, Rust traits, `std::string`, `Vec`,
Python objects, JavaScript objects, or mutable language-runtime internals.

Every buffer contract states:

- owner;
- valid lifetime;
- retain operation, when supported;
- release operation; and
- required release thread or executor.

Copy mode is the default: if an SDK does not explicitly guarantee that a
buffer may outlive its callback, the adapter copies it before the callback
returns. Retain/Release mode is allowed only when the SDK documents a
thread-safe retain/release contract. A buffer that must be released on its
origin thread is posted back to the Adapter queue; Rust does not release it
directly. Borrowed references never survive a callback, queue handoff, or FFI
call.

No C++ exception or Rust panic may cross the ABI. ABI entry points validate
versions, sizes, nullability, lengths, alignment, numeric overflow, and handle
state before use.

## 9. Thread and execution contract

RTC and native SDK callback threads may perform only bounded validation, frame
wrapping or required copying, timestamp capture, and non-blocking enqueue. They
must not execute graph scheduling, Node code, Python, TypeScript, blocking
network I/O, or an unbounded allocation loop.

Rust graph workers, adapter workers, Python execution domains, TypeScript
execution environments, and managed network I/O executors are separate
boundaries. Queue and admission behavior is owned by the Runtime rather than
reimplemented by business nodes.

## 10. Shutdown contract

Stop is idempotent and callable from any thread. The required order is:

1. Transition the Graph to `Stopping` and reject new work.
2. Broadcast cancellation, close or stop queues, and wake blocked participants.
3. Stop Source production.
4. Prevent new Adapter/SDK callbacks.
5. Wait with bounded diagnostics for in-flight callbacks to exit.
6. Destroy the Adapter and external SDK.
7. Wait for graph workers and managed tasks to exit.
8. Call reverse-topological `on_finish` for normal completion or `on_abort`
   once for failure/cancellation.
9. Release Core resources only after all execution tasks have exited.

Each edge declares whether stop drains or discards queued frames. No path may
free callback-visible state before in-flight callbacks have exited. Queue
closure wakes producers and consumers; shutdown cannot rely on busy waiting or
an unbounded silent join.

## 11. Signal and NotificationBus boundaries

A `SignalFrame` travels only to connected adjacent nodes and is delivered by a
queue, never by a direct cross-thread call. It is appropriate for pressure,
resume, and local control semantics.

An `EventFrame` emitted on an Event port remains normal Graph data. A process-local
notification is published through `NotificationBus` using publish, subscribe, and
unsubscribe operations; its typed envelope currently reuses `EventFrame`, but it does
not enter an Event output port or Edge queue. Slow subscribers cannot block Frame or
Signal data flow. NotificationBus is appropriate for graph-wide state and
observability, not media transport or hidden mutable configuration.

Topics and signal names use namespaces and versioned payload schemas.

## 12. Logging and metrics

Logs are structured and include stable fields such as session, graph, node,
edge, phase, error code, and trace identity when applicable. Default logging
must not emit media payloads, credentials, private extensions, or unrestricted
user text.

Minimum metrics include processing and error totals, processing duration,
queue capacity and length, high-water mark, enqueue/dequeue/drop/full totals,
blocked duration, oldest-frame age, discarded media duration, and last failure
reason. Edge metrics are keyed by Edge ID. High-cardinality Frame IDs, raw user
values, and arbitrary error messages are not metric labels.

Logging and metrics backends are replaceable. Their failure must not crash or
block a media path.

## 13. API stability and versioning

The project follows Semantic Versioning. During `0.x`, breaking changes remain
possible, but every public breaking change requires an explicit pre-release
note, affected-surface list, and migration guidance.

Rust APIs, JSON graph schema, C ABI, Python API, and Node-API surface are
versioned independently and published with a compatibility matrix. JSON uses a
root `schema_version`. The C ABI uses an ABI version, structure-size fields,
and capability queries so callers never infer support from memory layout.

Before 1.0, implementation details are not stable. Public contract changes
must nevertheless be detectable, documented, and testable. Compatibility is
not claimed merely because source code compiles.

License, governance, contributor covenant, security reporting policy, and
release signing are release-engineering decisions that must be resolved before
the first public release; they do not alter the runtime contract in Stage 1.

## 14. Planned repository boundaries

```text
crates/muxiva-core     Runtime graph, scheduling, lifecycle, flow, stop, metrics
crates/muxiva-types    Stable Rust frame, ID, value, error, and graph data types
crates/muxiva-cli      Validation, execution, and local Studio launcher
crates/muxiva-ffi      Versioned C ABI and safe Rust boundary
crates/muxiva-python   PyO3 node development surface
crates/muxiva-node     Node-API TypeScript/JavaScript surface
cpp/include         Public C ABI headers and C++ safety wrappers
cpp/nodes           C++ node examples and support
cpp/adapters        Replaceable native SDK adapters
studio              Local web graph editor
examples            Runnable reference graphs
docs                Design decisions and pre-release notes
tests               Cross-package, compatibility, fault, and quality suites
```

Directories are introduced only by their owning stage. Core must not depend on
binding, Studio, adapter, or example packages.

## 15. Stage gates

### Stage 1: scope, terminology, and contract

Input: the product goals and hard constraints.

Output: README, this contract, and foundation pre-release notes.

Exit: documents are internally consistent, use normative terminology, state
scope and non-goals, and define every later stage's boundary.

### Stage 2: Rust workspace and foundation types

Input: Stage 1 contract.

Output: Edition 2021 Cargo workspace, `muxiva-core`, `muxiva-types`, examples,
typed IDs, timestamps, contextual errors, replaceable logging, CI, and tests.

Exit: fmt, clippy, tests, and example builds pass; no Tokio, FFI, media SDK, or
unjustified unsafe is present.

### Stage 3: frames and ownership

Input: Stage 2 stable foundation types and Stage 1 ownership rules.

Output: immutable Frame variants and buffers, Value, extensions, lineage,
validation, Copy/Retain design, and ownership tests.

Exit: invalid dimensions and overflow are rejected; clone, concurrent-read,
release, privacy serialization, extension preservation, and lineage tests pass.

### Stage 4: lifecycle, graph, and synchronous runner

Input: Stage 3 Frame contract.

Output: Node lifecycle, ports, edges, policies, GraphDefinition, GraphBuilder,
validation, topological execution, mock text graph, and tests.

Exit: lifecycle ordering, graph errors, exact types, policies, abort idempotency,
panic translation, and resource cleanup are deterministic and tested. No real
concurrency, network, binding, or media SDK is introduced.

### Stage 5: concurrent runtime and safe stop

Input: deterministic Stage 4 runner and policies.

Output: bounded edge queues, per-node execution, admission, realtime contracts,
adaptive flow control, managed async streams, cancellation, draining, stop,
edge metrics, voice-path mocks, and concurrency tests.

Exit: slow consumers, pressure prediction, overflow policies, zero-loss tests,
session isolation, network isolation, and shutdown tests pass without busy wait,
silent loss, or unbounded queues.

### Stage 6: Signal, NotificationBus, and resources

Input: Stage 5 queue, worker, and cancellation boundaries.

Output: queued Signal routing, non-blocking process-local NotificationBus, typed ResourceStore,
opaque control delivery, and tests. Business Turn and interruption policy belongs
to Nodes, not Core.

Exit: routing scope, ordering, unsubscribe, slow subscribers, resource type
errors, queue bounds, and stop races are tested.

No stage may implement a later stage's output before its own exit criteria pass.

## 16. Stage 1 acceptance checklist

- [x] v0.1 scope and explicit non-goals are defined.
- [x] Voice-agent vertical validation is separated from Core semantics.
- [x] Node, Stream, Graph, Frame, Edge, Port, Adapter, and Stage are defined.
- [x] Planned directory ownership is documented.
- [x] GraphBuilder, JSON, Registry, CLI, and Studio share GraphDefinition.
- [x] Rust, C++, Adapter, Python, and TypeScript responsibilities are separated.
- [x] Frame header, timestamp, sequence, error, log, and metric minima are fixed.
- [x] Ownership, callback thread, stop order, and on_abort rules are normative.
- [x] API stability and SemVer policy are stated.
- [x] Inputs, outputs, and exit gates for Stages 2 through 6 are defined.

Document-level verification commands for Stage 1:

```sh
test -f README.md
test -f docs/design/01-product-and-technical-contract.md
test -f docs/pre_release_notes/01-foundation.md
rg -n "[T]ODO|[T]BD|[F]IXME" README.md docs
git diff --check
```

The placeholder scan must return no matches. Runtime tests begin in Stage 2;
Stage 1 contains no executable framework logic.
