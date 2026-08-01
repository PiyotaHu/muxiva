# Voxa

> A Rust-native, real-time multimodal agent runtime with one graph and lifecycle contract across Rust, C++, Python, and TypeScript.

[简体中文](README.zh-CN.md) · [Architecture](docs/design/01-product-and-technical-contract.md) · [Language SDKs](docs/sdk/README.md) · [Studio](docs/studio.md) · [Graph v1](docs/graph-v1-reference.md) · [Testing](docs/testing/README.md)

![Status](https://img.shields.io/badge/status-pre--alpha-orange)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![Rust](https://img.shields.io/badge/Rust-1.85%2B-black?logo=rust)
![C++](https://img.shields.io/badge/C%2B%2B-17-blue?logo=cplusplus)
![Python](https://img.shields.io/badge/Python-3.13-tested-blue?logo=python)
![Node.js](https://img.shields.io/badge/Node.js-22-tested-green?logo=nodedotjs)

Voxa is an early-stage runtime for building streaming voice, video, text, and binary agents as static processing graphs. Rust owns scheduling, bounded queues, backpressure, lifecycle, cancellation, signals, events, shutdown, and observability. Nodes and adapters can be implemented in Rust, C++, Python, or TypeScript without moving language-specific objects across runtime boundaries.

The project currently provides a tested foundation and mock integrations. It is not yet a production-ready agent platform.

## Why Voxa

- **One runtime core:** scheduling and safety semantics live in Rust.
- **One data model:** immutable `Frame` values carry audio, video, text, bytes, signals, and events.
- **Bounded by design:** queues, media duration, bytes, in-flight work, and shutdown deadlines have explicit limits.
- **Language isolation:** C ABI handles, Python execution domains, and Node.js workers prevent foreign code from running on RTC or Rust scheduling threads.
- **Deterministic lifecycle:** prepare, process, finish, abort, cancellation, and late-result behavior are explicit and tested.
- **One graph protocol:** programmatic builders, JSON Graph v1, the CLI, and the local Studio share the same graph definition.

## Project status

Voxa is **pre-alpha**. Stages 1–11 of the foundation plan are implemented, but several public APIs and integrations remain intentionally limited.

| Area | Status | Current boundary |
| --- | --- | --- |
| Frames, graph model, sync/concurrent runtime | Available | Static DAGs; exact port and frame types |
| Backpressure and real-time flow control | Available | Bounded queues, audio merge, managed streams |
| Signal, EventBus, turn control | Available | Adjacent signals and isolated global events |
| C ABI and C++ SDK | Available | Versioned ABI, RAII wrappers, installable CMake package, and hosted Graph v1 text factories |
| RTC adapters | Experimental | Mock contract plus optional Agora C++ PCM16/I420 and Python audio providers; live credential certification remains |
| Media normalization | Experimental | Optional FFmpeg streaming audio resampling plus RGBA8/I420 scale and color conversion |
| Python/PyO3 package | Experimental | Dedicated thread/asyncio loop and hosted Graph v1 text factories; `isolated_process` is rejected |
| Node-API package | Experimental | Dedicated Worker and hosted Graph v1 text factories; Promise-returning transforms are rejected |
| JSON Graph v1 and CLI | Experimental | Exact-version Registry compilation, concurrent execution of compiled-in factories, bounded waits, initialization, and local Studio |
| Local Studio | Available | Bundled visual canvas, Node/Edge editing, validation and atomic save |
| Model providers | Planned | Not included in Core or the current build |

## Architecture

```mermaid
flowchart LR
    SDK["Rust / C++ / Python / TypeScript SDKs"] --> GD["GraphDefinition / JSON Graph v1"]
    GD --> RT["Rust Runtime"]
    RT --> Q["Bounded Edge Queues"]
    Q --> N["Source / Transform / Sink Nodes"]
    RTC["RTC or external callbacks"] --> IN["Bounded ExternalIngress"]
    IN --> RT
    RT --> CP["Signal · EventBus · Turn Control"]
    RT --> OBS["Metrics · Diagnostics · Test Probes"]
```

The runtime never treats ASR, LLM, TTS, transport, or codec behavior as Core responsibilities. Those capabilities belong in nodes and adapters.

## Quick start

### Prerequisites

- Rust stable, as pinned by [`rust-toolchain.toml`](rust-toolchain.toml)
- A C11/C++17 compiler and CMake 3.20+ for the native SDK checks
- Optional: CPython 3.13 with maturin for Python bindings
- Optional: Node.js 22 and pnpm for Node-API bindings

### Build and run the Rust example

```bash
git clone https://github.com/PiyotaHu/Voxa.git
cd voxa
cargo build --workspace
cargo run -p voxa-examples --bin text_graph
```

### Validate and run a graph

```bash
cargo run -p voxa-cli -- validate examples/graphs/text-uppercase.v1.json
cargo run -p voxa-cli -- run examples/graphs/text-uppercase.v1.json
```

`voxa validate` is side-effect free: it never creates or starts a Node. `voxa run`
compiles the graph against the built-in Registry, materializes every exact
Factory selection, and executes it through the concurrent Runtime. Runs have a
30-second default deadline; use `--timeout-ms` and `--shutdown-timeout-ms` to
set bounded execution and cleanup waits.

### Start the local visual Studio

```bash
cargo run -p voxa-cli -- studio examples/graphs/text-uppercase.v1.json
```

Studio opens a bundled visual Graph v1 editor with a Node palette, SVG canvas, Inspector, Edge editor, diagnostics, JSON source view, Undo/Redo, and atomic save. It listens on `127.0.0.1` by default and generates a local access token. Binding a non-loopback address requires an explicit `--host` and prints a security warning. See the [Studio guide](docs/studio.md).

### Build and test the language SDKs

```bash
./scripts/check-python.sh
./scripts/check-node.sh
./scripts/check-ffi.sh
```

These scripts build real installable packages, run integration tests, and execute independent Python, TypeScript, and C++ consumer examples. See the [language SDK guide](docs/sdk/README.md) for installation and Node development examples.

## Graph v1 example

```json
{
  "version": "voxa.graph/v1",
  "graph_id": "text-uppercase",
  "nodes": [
    {
      "id": "source",
      "node_type": "builtin.text_source",
      "language": "rust",
      "factory_version": "1.0.0",
      "node_config": { "text": "hello" }
    },
    {
      "id": "upper",
      "node_type": "builtin.uppercase",
      "language": "rust",
      "factory_version": "1.0.0",
      "node_config": {}
    }
  ],
  "edges": [
    {
      "id": "source-upper",
      "from": { "node_id": "source", "port": "text_out" },
      "to": { "node_id": "upper", "port": "text_in" },
      "frame_type": "text",
      "queue_policy": { "capacity": 32, "overflow": "block" }
    }
  ]
}
```

Graph JSON is declarative configuration. It cannot contain executable code, dynamic scripts, credentials, or arbitrary remote resources. See the [Graph v1 reference](docs/graph-v1-reference.md).

## Repository layout

```text
voxa/
├── crates/
│   ├── voxa-types/       # Immutable frames, IDs, values, errors
│   ├── voxa-core/        # Graph, runtime, queues, flow and control plane
│   ├── voxa-ffi/         # Versioned C ABI
│   ├── voxa-graph-json/  # Graph v1 parser and compiler
│   ├── voxa-cli/         # voxa command-line interface
│   ├── voxa-studio/      # Local token-authenticated Studio server
│   ├── voxa-python/      # PyO3/maturin package
│   ├── voxa-node/        # Node-API native module
│   └── voxa-testkit/     # Deterministic test harnesses
├── bindings/node/        # @voxa/core package
├── cpp/                  # Public C/C++ SDK, RTC adapters and media providers
├── examples/             # Rust, graph, Python, TypeScript and C++ examples
├── fuzz/                 # Fuzz targets
├── docs/                 # Design, testing and pre-release reports
└── scripts/              # Reproducible quality gates
```

## Quality gates

Run the consolidated local gate:

```bash
./scripts/check-quality.sh
```

Individual gates include:

```bash
./scripts/check-rust.sh
./scripts/check-ffi.sh
./scripts/check-ffi-asan.sh
./scripts/check-rtc.sh
./scripts/check-rtc-asan.sh
./scripts/check-python.sh
./scripts/check-node.sh
./scripts/check-cpp-consumer.sh
./scripts/check-bench.sh
```

The test framework covers deterministic graph faults, queue pressure, managed-stream cancellation, foreign execution domains, ABI ownership, Mock RTC shutdown, CLI/Studio authorization, and port conflicts. Optional Miri, fuzz, and TSan scripts report an explicit `SKIP` when the required toolchain is unavailable.

## Roadmap

Near-term priorities:

1. Stabilize public Rust, C++, Python, and TypeScript SDK contracts.
2. Stabilize the new schema-driven multimodal Source, Transform, Sink, and named multi-output foreign Factory APIs.
3. Stabilize Studio live runtime metrics and execution-control contracts.
4. Certify the Agora adapter with live-room soak tests and extend D08 into compressed codec/device providers.
5. Implement versioned Python process isolation and TypeScript Promise support.
6. Publish packages, compatibility matrices, performance baselines, and release artifacts.

Real provider integrations should remain adapters or nodes and must not become mandatory Core dependencies.

## Contributing

Design feedback, bug reports, reproducible test cases, and focused pull requests are welcome. Before changing runtime contracts, read the [product and technical contract](docs/design/01-product-and-technical-contract.md) and the [testing guide](docs/testing/README.md).

Please keep changes bounded, deterministic, and free of real service credentials. New foreign-language, RTC, or network integrations must include ownership, threading, cancellation, late-callback, and shutdown tests.

Dedicated `CONTRIBUTING.md`, Code of Conduct, issue templates, and pull-request templates are planned before the first public release.

## Security

Voxa is pre-alpha and must not be used to execute untrusted code or expose Studio directly to the public internet. Graph files must never contain secrets. Use local credential references and keep Studio on its default loopback address.

GitHub private vulnerability reporting and a dedicated `SECURITY.md` should be enabled before public deployment.

## License

Voxa is licensed under the [Apache License 2.0](LICENSE).
