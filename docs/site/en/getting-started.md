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

## First run: a real voice assistant

Voxa's primary developer experience is the credentialed Qwen + Agora Voice
Room, not synthetic ASR, LLM, or TTS output. After preparing an Agora Native
C++ SDK, three short-lived RTC tokens, and DashScope credentials, continue with
the [flagship voice demo](voice-demo.md).

```bash
./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk
./examples/voice-agent/run.sh
```

## Create and run a graph

```bash
voxa init my-agent.voxa.json
voxa validate my-agent.voxa.json
voxa run my-agent.voxa.json
```

`validate` is side-effect free. `run` compiles the Graph against the exact Node
Registry, materializes selected Factories, and executes them through the
concurrent Runtime with bounded execution and shutdown deadlines.

## Open Studio

```bash
voxa studio my-agent.voxa.json
```

Studio opens locally with a random access token. Continue with the
[Studio guide](studio.md).
