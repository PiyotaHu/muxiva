# Testing

Voxa treats deterministic failure behavior as part of the public contract.

## Local gates

```bash
./scripts/check-quality.sh
```

Focused checks include Rust, Python, Node.js, C ABI, C++ consumers, RTC, media,
sanitizers, benchmarks, fuzzing, Miri, and Studio browser tests.

## Offline Runtime simulator

`voxa simulate` and `examples/graphs/mock-realtime-voice.v1.json` are network-free,
credential-free Runtime fixtures. They use synthetic PCM and scripted text to
test fork/join routing, backpressure, Signals, EventBus, turns, and lifecycle.
They provide no real ASR, LLM, or TTS and are not a product demo.

```bash
voxa simulate --turns 4
voxa studio examples/graphs/mock-realtime-voice.v1.json
```

## Continuous integration

Protected pull requests require:

- Rust formatting, Clippy with warnings denied, and workspace tests;
- Python tests on Ubuntu and macOS;
- Node.js tests on Ubuntu and macOS;
- native FFI, RTC, and media checks on Ubuntu and macOS;
- CODEOWNERS review and resolved review conversations.

Scheduled or additional workflows cover sanitizers, Miri, fuzzing, benchmarks,
and strict documentation builds.

## What tests must prove

- exact Graph and port validation;
- queue capacity and overflow behavior;
- cancellation and idempotent shutdown;
- late results and late foreign callbacks;
- ownership and buffer lifetime across ABI boundaries;
- no work on forbidden RTC or scheduler threads;
- stable diagnostics and retained terminal metrics;
- bounded time, memory, bytes, and in-flight work.

!!! note
    A check that reports `SKIP` is not equivalent to tested behavior. Missing
    toolchains and credentials must be visible in certification results.
