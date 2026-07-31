# Voxa

Voxa is an open-source, real-time multimodal agent runtime. It uses a single
Rust core for graph execution, scheduling, queues, backpressure, lifecycle,
signals, events, shutdown, and observability while allowing nodes to be written
in Rust, C++, Python, and TypeScript.

Voxa is currently in its pre-release foundation phase. The first vertical
validation target is a real-time voice agent built from mock nodes:

```text
MockAudioSource -> MockAsr -> MockLlm -> MockTts -> AudioSink
```

ASR, LLM, TTS, and transport behavior belong to nodes and adapters, not to the
core. The runtime contract remains equally applicable to audio, video, text,
and binary streams.

## Project principles

- Rust is the only runtime core.
- A `Frame` is the only information unit exchanged between nodes.
- Graphs are static directed acyclic graphs in the v0.1 MVP.
- Node lifecycle is limited to `on_prepare`, `on_process`, `on_finish`, and
  `on_abort`.
- Cross-language boundaries use versioned C-compatible data, error codes, and
  opaque handles. Language implementation objects never cross the boundary.
- Real-time SDK callback threads only validate, wrap, and enqueue data.
- Every buffer has an explicit owner, lifetime, release operation, and release
  thread requirement.
- Shutdown and abort behavior are deterministic and idempotent.

## v0.1 MVP

The v0.1 MVP will provide:

- `SourceNode`, `TransformNode`, and `SinkNode` in a static DAG;
- a programmatic `GraphBuilder` and a serializable JSON `GraphDefinition`;
- one graph protocol shared by the runtime, CLI, and local web Studio;
- `AudioFrame`, `VideoFrame`, `TextFrame`, and `ByteFrame` data frames;
- adjacent-node `SignalFrame` routing and a global `EventFrame` bus;
- multithreaded streaming, basic backpressure, error propagation, safe stop,
  logs, and metrics;
- C++, Python, and TypeScript node development surfaces; and
- a mock transport reference voice-agent graph.

The MVP does not include real RTC, FFmpeg, dynamic plugins, Java, GPU
execution, or distributed scheduling. Python async support is restricted to
controlled I/O workloads; CPU-heavy work cannot be presented as async.

## Status and documentation

Stage 3 frames and ownership is implemented and awaiting acceptance. Stage 4
has not started. By maintainer direction, the remaining Stage 2 review findings
and the non-blocking Stage 3 review findings are explicitly deferred; they
remain recorded in the pre-release reports.

- [Product and technical contract](docs/design/01-product-and-technical-contract.md)
- [Foundation pre-release notes](docs/pre_release_notes/01-foundation.md)
- [Stage 2 Rust foundation report](docs/pre_release_notes/02-rust-foundation.md)
- [Stage 3 frames and ownership report](docs/pre_release_notes/03-frames-and-ownership.md)

## Planned repository layout

```text
voxa/
├── Cargo.toml
├── crates/
│   ├── voxa-core/
│   ├── voxa-types/
│   ├── voxa-cli/
│   ├── voxa-ffi/
│   ├── voxa-python/
│   └── voxa-node/
├── cpp/
│   ├── include/
│   ├── nodes/
│   └── adapters/
├── studio/
├── examples/
├── docs/
│   ├── design/
│   └── pre_release_notes/
└── tests/
```

Directories are created only when their owning stage begins. This tree defines
responsibility boundaries; it is not evidence that later-stage functionality
already exists.

## Contributing

Voxa is not ready for external implementation contributions yet. During the
foundation stages, design discussion and review should use the terminology and
hard constraints in the technical contract. Contribution policy, code of
conduct, security policy, and license selection will be finalized before the
first public release.
