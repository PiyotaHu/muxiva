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

Stages 3 through 10 are implemented and awaiting acceptance. Stage 10 adds a
bounded Graph v1 JSON parser/compiler, local CLI, and token-authenticated local
Studio HTTP foundation. Stage 9 adds
bounded Python/PyO3 and TypeScript/Node-API language execution domains over a
shared Rust-owned foreign driver. Stage 8 adds a
Rust-owned bounded external ingress plus a versioned C++ mock RTC adapter with
copy-only media delivery, deterministic faults, and callback-safe shutdown.
Stage 7 adds the
versioned copy-owned C ABI, generation-checked handles, C++ RAII/node
trampolines, and a focused C++ transform running inside a Rust graph. Stage 6 adds
bounded adjacent Signal routing, an isolated global EventBus, typed graph
resources, and atomic transport/turn control with stale-frame Sink filtering.
Stage 5's report records its intentionally incomplete runtime/UI integration,
full-pipeline-drain and real-transport boundaries. By
maintainer direction, the remaining Stage 2 review findings and the
non-blocking Stage 3 and 4 review findings are explicitly deferred; they
remain recorded in the pre-release reports.

- [Product and technical contract](docs/design/01-product-and-technical-contract.md)
- [Foundation pre-release notes](docs/pre_release_notes/01-foundation.md)
- [Stage 2 Rust foundation report](docs/pre_release_notes/02-rust-foundation.md)
- [Stage 3 frames and ownership report](docs/pre_release_notes/03-frames-and-ownership.md)
- [Stage 4 synchronous graph runtime report](docs/pre_release_notes/04-node-graph-sync-runner.md)
- [Stage 5 concurrent runtime and flow-control report](docs/pre_release_notes/05-concurrent-runtime-flow-control.md)
- [Stage 6 Signal/EventBus/turn-control design](docs/design/06-signal-eventbus-turn-control.md)
- [Stage 6 pre-release report](docs/pre_release_notes/06-signal-eventbus-turn-control.md)
- [Stage 7 C ABI and C++ SDK design](docs/design/07-c-abi-cpp-node-sdk.md)
- [Stage 7 pre-release report](docs/pre_release_notes/07-c-abi-cpp-node-sdk.md)
- [Stage 8 mock RTC adapter design](docs/design/08-mock-rtc-adapter.md)
- [Stage 8 pre-release report](docs/pre_release_notes/08-mock-rtc-adapter.md)
- [Stage 9 Python and Node execution-domain design](docs/design/09-python-node-execution-domains.md)
- [Stage 9 pre-release report](docs/pre_release_notes/09-python-node.md)
- [Stage 10 Graph v1, CLI, and Studio design](docs/design/10-graph-json-cli-studio.md)
- [Stage 11 testing and quality gates](docs/testing/README.md)
- [Stage 11 deterministic fault matrix](docs/testing/fault-injection.md)
- [Stage 11 pre-release report](docs/pre_release_notes/11-test-quality.md)

The Stage 7 developer check needs Cargo plus a C11/C++17 compiler; CMake is
not required:

```sh
cargo test --workspace --offline
CC=clang CXX=clang++ ./scripts/check-ffi.sh
./scripts/check-rtc.sh
./scripts/check-rtc-asan.sh
./scripts/check-python.sh
./scripts/check-node.sh
cargo test --offline -p voxa-studio -p voxa-cli
```

The binding scripts build real importable packages and require their local
Python/Node tools and dependency caches. The Studio tests use loopback sockets;
they require no browser, external network, or service credentials. See the
[testing guide](docs/testing/README.md) for exact prerequisites and coverage.

The Stage 9 Rust test gate must select a supported arm64 Python explicitly on
this development host because its default `python3` shim is legacy x86_64:

```sh
PYO3_PYTHON=/Users/private-user/.pyenv/versions/3.13.12/bin/python3.13 \
  cargo test --workspace --offline
```

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
