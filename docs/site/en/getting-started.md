# Installation and first run

Muxiva is pre-alpha. The repository currently provides source installation while
the release pipeline for standalone binaries, Wheels, npm packages, and native
SDK archives is being completed.

## Prerequisites

- Git;
- the Rust toolchain pinned by `rust-toolchain.toml`;
- CMake 3.20+ and a C11/C++17 compiler for native development;
- optional CPython and maturin for Python;
- optional Node.js and pnpm for TypeScript.

## Install the CLI once

```bash
git clone https://github.com/PiyotaHu/muxiva.git
cd Muxiva
cargo install --locked --path crates/muxiva-cli
muxiva --version
```

After installation, application developers use `muxiva`; they do not run the
workspace through `cargo run` for normal workflows.

## CLI entry point

Running `muxiva` alone shows three recommended entry points. `muxiva --help`
explains every command:

| Command | Purpose |
| --- | --- |
| `muxiva studio [project or graph]` | Open Studio; auto-discover or create a workspace when omitted |
| `muxiva init [directory]` | Create a complete project with `graph.json` and `.muxiva/` |
| `muxiva validate <project or graph>` | Validate without creating or executing Nodes |
| `muxiva run <project or graph>` | Execute a Graph with the concurrent Runtime |
| `muxiva doctor [--voice]` | Check tools, project discovery, and real-voice readiness |
| `muxiva simulate` | Run synthetic, network-free Runtime fixtures; not a product demo |

## First run: a real voice assistant

Muxiva's primary developer experience is the credentialed Qwen + Agora Voice
Room, not synthetic ASR, LLM, or TTS output. On macOS the setup command
downloads and verifies Agora automatically; Qwen requires no SDK download.
Follow the [from-scratch flagship guide](voice-demo.md) to create the browser and Bot RTC
tokens, Model Studio API Key, and Workspace ID.

```bash
./examples/voice-agent/setup.sh
./examples/voice-agent/run.sh
```

## Create and run a graph

```bash
muxiva init my-agent
muxiva validate my-agent
muxiva run my-agent
```

`init` creates `my-agent/graph.json`, `.muxiva/nodes/`, `.muxiva/templates/`, and a
project README. `validate` is side-effect free. `run` compiles the Graph against
the exact Node Registry, materializes selected Factories, and executes them
through the concurrent Runtime with bounded execution and shutdown deadlines.
The older single-`.json` form remains compatible.

## Open Studio

```bash
cd my-agent
muxiva studio
```

Studio discovers `graph.json` automatically. Outside a project it safely
creates `muxiva.graph.json`; from the Muxiva source root it discovers the flagship
Voice Agent workspace. It binds locally with a random access token. Continue with the
[Studio guide](studio.md).
