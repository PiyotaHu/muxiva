# Muxiva

> A Rust-native, real-time multimodal agent runtime with one graph and lifecycle contract across Rust, C++, Python, and TypeScript.

[简体中文](README.zh-CN.md) · [Documentation](https://piyotahu.github.io/muxiva/) · [Architecture](https://piyotahu.github.io/muxiva/concepts/) · [Flagship voice demo](https://piyotahu.github.io/muxiva/voice-demo/) · [Developer manual](https://piyotahu.github.io/muxiva/nodes/) · [Agent integration](https://piyotahu.github.io/muxiva/nodes/agent-integration/) · [Studio](https://piyotahu.github.io/muxiva/studio/) · [Observability](https://piyotahu.github.io/muxiva/observability/) · [Graph v1](https://piyotahu.github.io/muxiva/graph/) · [Testing](https://piyotahu.github.io/muxiva/testing/)

![Status](https://img.shields.io/badge/status-pre--alpha-orange)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
[![CI](https://github.com/PiyotaHu/muxiva/actions/workflows/ci.yml/badge.svg)](https://github.com/PiyotaHu/muxiva/actions/workflows/ci.yml)
[![Bindings](https://github.com/PiyotaHu/muxiva/actions/workflows/bindings.yml/badge.svg)](https://github.com/PiyotaHu/muxiva/actions/workflows/bindings.yml)
[![Documentation](https://github.com/PiyotaHu/muxiva/actions/workflows/docs.yml/badge.svg)](https://piyotahu.github.io/muxiva/)
![Rust](https://img.shields.io/badge/Rust-1.85%2B-black?logo=rust)
![C++](https://img.shields.io/badge/C%2B%2B-17-blue?logo=cplusplus)
![Python](https://img.shields.io/badge/Python-3.13-tested-blue?logo=python)
![Node.js](https://img.shields.io/badge/Node.js-22-tested-green?logo=nodedotjs)

Muxiva is an early-stage runtime for building streaming voice, video, text, and binary agents as static processing graphs. Rust owns scheduling, bounded queues, backpressure, lifecycle, cancellation, signals, events, shutdown, and observability. Nodes and adapters can be implemented in Rust, C++, Python, or TypeScript without moving language-specific objects across runtime boundaries.

The project currently provides a tested Runtime foundation and an
application-layer Qwen + Agora real-voice flagship. It is not yet a
production-ready agent platform.

## Why Muxiva

- **One runtime core:** scheduling and safety semantics live in Rust.
- **One data model:** immutable `Frame` values carry audio, video, text, bytes, signals, and events.
- **Bounded by design:** queues, media duration, bytes, in-flight work, and shutdown deadlines have explicit limits.
- **Language isolation:** C ABI handles, Python execution domains, and Node.js workers prevent foreign code from running on RTC or Rust scheduling threads.
- **Deterministic lifecycle:** prepare, process, finish, abort, cancellation, and late-result behavior are explicit and tested.
- **One graph protocol:** programmatic builders, JSON Graph v1, the CLI, and the local Studio share the same graph definition.

## Project status

Muxiva is **pre-alpha**. Stages 1–11 of the foundation plan are implemented, but several public APIs and integrations remain intentionally limited.

| Area | Status | Current boundary |
| --- | --- | --- |
| Frames, graph model, sync/concurrent runtime | Available | Static DAGs; exact port and frame types |
| Backpressure and real-time flow control | Available | Bounded queues, audio merge, managed streams |
| Signal and NotificationBus control | Available | Explicit adjacent Signal routing; process-local observable Events |
| C ABI and C++ SDK | Available | Versioned ABI, RAII wrappers, installable CMake package, and hosted Graph v1 text factories |
| RTC Nodes | Experimental | Shared-session Agora C++ audio/data ingress and egress; live credential certification remains |
| Media normalization | Experimental | Optional FFmpeg streaming audio resampling plus RGBA8/I420 scale and color conversion |
| Python/PyO3 package | Experimental | Dedicated thread/asyncio loop and hosted Graph v1 text factories; `isolated_process` is rejected |
| Node-API package | Experimental | Dedicated Worker and hosted Graph v1 text factories; Promise-returning transforms are rejected |
| JSON Graph v1 and CLI | Experimental | Exact-version Registry compilation, concurrent execution of compiled-in factories, bounded waits, initialization, and local Studio |
| Local Studio | Available | Node Lab, typed wiring, Python Host, C++ ABI packs, project experiences, local Run/Stop |
| Model Nodes | Experimental | Qwen Python Node Packs are vendor adapters outside Core |

## Architecture

[![Muxiva system architecture](docs/site/en/assets/architecture/muxiva-system-overview.png)](https://piyotahu.github.io/muxiva/concepts/)

Read the diagram from top to bottom: product surfaces declare a Graph and discover
Node Factories; the vendor-neutral Rust Core compiles and executes it; Rust, C++,
Python, and TypeScript Nodes provide replaceable capabilities; RTC, model APIs, and
token services remain outside Core. Solid lines are data or calls, dashed magenta
lines are Signal control, and dotted gray lines are process-local NotificationBus telemetry.

The runtime never treats ASR, LLM, TTS, transport, codec behavior, or a vendor
“provider” as Core responsibilities. See the
[system overview and core-concepts walkthrough](https://piyotahu.github.io/muxiva/concepts/),
or open the [editable Draw.io source](docs/site/en/assets/architecture/muxiva-system-overview.drawio).

## Quick start

### Prerequisites

- Rust stable, as pinned by [`rust-toolchain.toml`](rust-toolchain.toml)
- A C11/C++17 compiler and CMake 3.20+ for the native SDK checks
- Optional: CPython 3.13 with maturin for Python bindings
- Optional: Node.js 22 and pnpm for Node-API bindings; Demo 2's Pi Agent requires Node.js 22.19+

### Install the `muxiva` CLI once

```bash
git clone https://github.com/PiyotaHu/muxiva.git muxiva
cd muxiva
cargo install --locked --path crates/muxiva-cli
muxiva --version
```

Until the first binary release, installation builds the CLI from the checkout.
After that one-time step, normal usage never needs `cargo run -p ...` or
knowledge of the Rust workspace.

### Run the real voice assistant

The flagship application offers Qwen Audio Realtime plus **Demo 2**, an
inspectable full-duplex Qwen Server VAD + ASR → independently versioned Pi
TypeScript coding Agent → cancellable Qwen TTS graph, with Agora C++ transport,
a browser microphone, and real workspace-scoped file tools:

```bash
./examples/voice-agent/setup.sh       # macOS: downloads and verifies Agora SDK
./examples/voice-agent/run.sh
```

Choose a graph in Studio, fill **Connections**, click **Run**, then open **Voice Room**. The
full setup, two-identity shared-session RTC model, security boundary, and offline gates
are documented in the [flagship voice demo guide](https://piyotahu.github.io/muxiva/voice-demo/).

### Create, validate, and run a graph

```bash
muxiva init my-agent
muxiva validate my-agent
muxiva run my-agent
```

`muxiva init` creates a complete project directory. `muxiva validate` is side-effect free: it never creates or starts a Node. `muxiva run`
compiles the graph against the built-in Registry, materializes every exact
Factory selection, and executes it through the concurrent Runtime. Runs have a
30-second default deadline; use `--timeout-ms` and `--shutdown-timeout-ms` to
set bounded execution and cleanup waits.

### Start the local visual Studio

```bash
muxiva studio
```

With no argument, Studio discovers the current project; from the Muxiva source
root it opens the flagship Voice Agent. Studio opens a bundled visual Graph v1 editor. Drag Nodes from the Palette,
wire compatible typed ports, open **◎ Observe** to identify slow Nodes and backed-up Edges, or open **Create
Node** to edit and register a project Node without leaving the browser. Text
Python project Nodes run through the trusted local development Host. Studio
listens on `127.0.0.1` by default and generates a local access token. See the
[Studio guide](https://piyotahu.github.io/muxiva/studio/) and
[observability guide](https://piyotahu.github.io/muxiva/observability/).

### Build and test the language SDKs

```bash
./scripts/check-python.sh
./scripts/check-node.sh
./scripts/check-ffi.sh
```

These scripts build real installable packages, run integration tests, and execute independent Python, TypeScript, and C++ consumer examples. See the [developer manual](https://piyotahu.github.io/muxiva/nodes/) for Agent integration and language-specific Node workflows.

## Flagship graphs

The real-voice Realtime and Cascade templates live under
[`examples/voice-agent/.muxiva/templates/`](examples/voice-agent/.muxiva/templates/).
Start Studio with `./examples/voice-agent/run.sh` to select, inspect, and edit
either graph.

Graph JSON is declarative configuration. It cannot contain executable code, dynamic scripts, credentials, or arbitrary remote resources. See the [Graph and typed ports guide](https://piyotahu.github.io/muxiva/graph/).

## Repository layout

```text
muxiva/
├── crates/
│   ├── muxiva-types/       # Immutable frames, IDs, values, errors
│   ├── muxiva-core/        # Graph, runtime, queues, flow and control plane
│   ├── muxiva-ffi/         # Versioned C ABI
│   ├── muxiva-graph-json/  # Graph v1 parser and compiler
│   ├── muxiva-cli/         # muxiva command-line interface
│   ├── muxiva-studio/      # Local token-authenticated Studio server
│   ├── muxiva-python/      # PyO3/maturin package
│   ├── muxiva-node/        # Node-API native module
│   └── muxiva-testkit/     # Deterministic test harnesses
├── bindings/node/        # @muxiva/core package
├── bindings/agent/       # Vendor-neutral @muxiva/agent TypeScript contract
├── cpp/                  # Public C/C++ SDK
├── providers/            # Vendor integrations: Qwen/Python and Agora/C++
├── examples/             # Rust, graph, Python, TypeScript and C++ examples
├── fuzz/                 # Fuzz targets
├── docs/                 # Design, testing and pre-release reports
└── scripts/              # Reproducible quality gates
```

## Quality gates

The commands below are for Muxiva contributors working on the repository, not
for application developers using the installed `muxiva` binary.

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
3. Stabilize Studio observability thresholds and certify Prometheus/OTLP compatibility with hosted backends.
4. Run and retain D09 Agora live-room certification on each release platform; extend D08 into compressed codec/device providers.
5. Implement versioned Python process isolation and TypeScript Promise support.
6. Publish packages, compatibility matrices, performance baselines, and release artifacts.

Real provider integrations should remain adapters or nodes and must not become mandatory Core dependencies.

## Contributing

Design feedback, bug reports, reproducible test cases, and focused pull requests
are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md), the
[Code of Conduct](CODE_OF_CONDUCT.md), and the
[governance model](GOVERNANCE.md) before participating. Public API, Graph or
Manifest Schema, Runtime, Studio, CLI, provider, and architecture changes must
update `docs/` in the same pull request.

Please keep changes bounded, deterministic, and free of real service credentials. New foreign-language, RTC, or network integrations must include ownership, threading, cancellation, late-callback, and shutdown tests.

## Security

Muxiva is pre-alpha and must not be used to execute untrusted code or expose
Studio directly to the public internet. Graph files must never contain secrets.
Report vulnerabilities privately according to [SECURITY.md](SECURITY.md).

See [CHANGELOG.md](CHANGELOG.md) for notable unreleased changes and
[SUPPORT.md](SUPPORT.md) for help channels and report requirements.

## License

Muxiva is licensed under the [Apache License 2.0](LICENSE).
