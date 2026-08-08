# Contributing to Muxiva

Thank you for helping build a safe, real-time multimodal agent runtime. Muxiva is
pre-alpha: focused bug reports, design reviews, tests, documentation, and small
well-scoped pull requests are especially valuable.

By participating, you agree to follow our [Code of Conduct](CODE_OF_CONDUCT.md).
Security vulnerabilities must follow [SECURITY.md](SECURITY.md), not a public
Issue.

## Before writing code

1. Search existing Issues and Discussions.
2. Open an Issue before a large API, ABI, Graph schema, scheduling, ownership,
   or provider change.
3. Read the [product contract](docs/design/01-product-and-technical-contract.md)
   and the relevant design document.
4. Keep providers, model vendors, codecs, and transports outside Runtime Core.

Small bug fixes, tests, and documentation corrections may go directly to a PR.

## Development setup

Required for the Rust workspace:

- the toolchain pinned by `rust-toolchain.toml`;
- Git;
- a C11/C++17 compiler and CMake 3.20+ for native checks;
- `clang-format` for C/C++ formatting.

Optional language environments:

- CPython and maturin for Python bindings;
- Node.js and pnpm for TypeScript/Node-API bindings;
- MkDocs dependencies from `requirements-docs.txt` for the documentation site.

```bash
git clone https://github.com/PiyotaHu/muxiva.git
cd Muxiva
./scripts/check-rust.sh
```

Use a focused branch such as `fix/queue-shutdown` or `feat/python-frame-port`.
Do not commit generated build directories, credentials, RTC tokens, or service
account files.

## Quality gates

Run the smallest relevant gate while developing, then the consolidated gate
before requesting review:

```bash
./scripts/check-quality.sh
```

Common focused gates:

```bash
./scripts/check-rust.sh
./scripts/check-python.sh
./scripts/check-node.sh
./scripts/check-ffi.sh
./scripts/check-cpp-consumer.sh
./scripts/check-studio-e2e.sh
```

Every behavior change needs a regression test at the lowest stable public
boundary. Foreign-language and native changes must cover ownership, threading,
cancellation, late callbacks, errors, and bounded shutdown.

## Documentation is part of the contract

A change is incomplete when its documentation is stale. Update the paired
public sources under `docs/site/en/` and `docs/site/zh/` in the same PR whenever
a change affects:

- public Rust, C, C++, Python, or TypeScript APIs;
- Graph v1, Node Manifest, ports, Frames, configuration, or lifecycle;
- Runtime scheduling, backpressure, cancellation, Signals, Events, or metrics;
- Studio workflows, CLI commands, installation, or provider behavior;
- architecture decisions, support boundaries, security, or compatibility.

Validate the public site locally:

```bash
python -m pip install -r requirements-docs.txt
python scripts/check-docs-i18n.py
mkdocs build --strict
```

Changes under `docs/**` are published automatically to
<https://piyotahu.github.io/muxiva/> after they merge to `main`.

## Pull requests

- Keep one logical change per PR.
- Explain the user-visible outcome and the invariant being preserved.
- Link the Issue or design discussion when one exists.
- Include tests, documentation, and compatibility notes.
- Avoid drive-by formatting or unrelated refactors.
- Keep commits reviewable; maintainers may squash on merge.

No Contributor License Agreement is currently required. Contributions are
accepted under the repository's [Apache-2.0 License](LICENSE).

## 中文说明

提交代码前请阅读相关设计文档，并运行对应质量门禁。任何公开 API、Graph/Node
Schema、Runtime 语义、Studio、CLI 或 Provider 行为变化，都必须在同一个 PR
同步更新 `docs/site/en/` 与 `docs/site/zh/`。漏洞请使用私密安全报告，禁止公开
提交包含凭据的 Issue。
