# Installation and first run

Voxa is pre-alpha. The repository currently provides source installation while
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
git clone https://github.com/PiyotaHu/Voxa.git
cd Voxa
cargo install --locked --path crates/voxa-cli
voxa --version
```

After installation, application developers use `voxa`; they do not run the
workspace through `cargo run` for normal workflows.

## CLI entry point

Running `voxa` alone shows three recommended entry points. `voxa --help`
explains every command:

| Command | Purpose |
| --- | --- |
| `voxa studio [project or graph]` | Open Studio; auto-discover or create a workspace when omitted |
| `voxa init [directory]` | Create a complete project with `graph.json` and `.voxa/` |
| `voxa validate <project or graph>` | Validate without creating or executing Nodes |
| `voxa run <project or graph>` | Execute a Graph with the concurrent Runtime |
| `voxa doctor [--voice]` | Check tools, project discovery, and real-voice readiness |
| `voxa simulate` | Run synthetic, network-free Runtime fixtures; not a product demo |

## First run: a real voice assistant

Voxa's primary developer experience is the credentialed Qwen + Agora Voice
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
voxa init my-agent
voxa validate my-agent
voxa run my-agent
```

`init` creates `my-agent/graph.json`, `.voxa/nodes/`, `.voxa/templates/`, and a
project README. `validate` is side-effect free. `run` compiles the Graph against
the exact Node Registry, materializes selected Factories, and executes them
through the concurrent Runtime with bounded execution and shutdown deadlines.
The older single-`.json` form remains compatible.

## Open Studio

```bash
cd my-agent
voxa studio
```

Studio discovers `graph.json` automatically. Outside a project it safely
creates `voxa.graph.json`; from the Voxa source root it discovers the flagship
Voice Agent workspace. It binds locally with a random access token. Continue with the
[Studio guide](studio.md).
