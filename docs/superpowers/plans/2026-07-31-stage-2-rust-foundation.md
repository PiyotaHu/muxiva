# Muxiva Stage 2 Rust Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a sustainable Rust Edition 2021 workspace with typed foundation values, contextual errors, replaceable logging, a runnable hello example, and Linux CI.

**Architecture:** `muxiva-types` owns dependency-light public value and error types; `muxiva-core` depends on those types and owns runtime-facing services such as logging; `muxiva-examples` depends on the public crates and proves consumer ergonomics. The stage adds no graph execution, Frame model, concurrency runtime, FFI, or media integration.

**Tech Stack:** Rust stable, Edition 2021, Cargo workspace, `thiserror`, `tracing`, `tracing-subscriber`, standard-library tests, GitHub Actions.

## Global Constraints

- Use Rust stable and Edition 2021; Linux is the primary supported platform.
- Workspace crates are `muxiva-core`, `muxiva-types`, and `muxiva-examples`.
- Do not add Tokio, async runtimes, cross-language bindings, Frame types, graph execution, or media dependencies.
- Do not use `unsafe` in Stage 2.
- IDs must be distinct strong types rather than aliases or unrestricted interchangeable strings.
- Public errors must carry a stable category, stable code, human-readable message, and structured context.
- Logging must expose a replaceable interface and a default `tracing` implementation.
- Every production behavior is introduced through a failing test first.
- Every task must leave `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --all-targets` passing once the relevant files exist.
- The local environment did not have `rustc` or `cargo` on `PATH` when this plan was written; install the stable Rust toolchain with `rustfmt` and `clippy`, or expose an existing installation, before Task 1 verification.

---

## Planned file structure

```text
Cargo.toml                              Workspace membership, dependency versions, metadata
rust-toolchain.toml                     Stable toolchain plus rustfmt and clippy
.rustfmt.toml                           Repository formatting policy
.gitignore                              Rust build output exclusions
.github/workflows/ci.yml                Linux format, lint, test, and example gates
crates/muxiva-types/Cargo.toml             Public foundation-type crate manifest
crates/muxiva-types/src/lib.rs             Public exports only
crates/muxiva-types/src/error.rs           ErrorCategory, ErrorContext, MuxivaError, Result
crates/muxiva-types/src/id.rs              SessionId, NodeId, StreamId, TraceId newtypes
crates/muxiva-types/src/time.rs            Timestamp and SequenceId value types
crates/muxiva-core/Cargo.toml              Core crate manifest
crates/muxiva-core/src/lib.rs              Core public exports only
crates/muxiva-core/src/logging.rs          LogSink abstraction and tracing implementation
crates/muxiva-examples/Cargo.toml          Example package manifest
crates/muxiva-examples/src/lib.rs          Testable hello-message construction
crates/muxiva-examples/src/bin/hello.rs    Runnable hello example
docs/pre_release_notes/02-rust-foundation.md  Stage report, APIs, checks, and risks
```

`lib.rs` files remain small export surfaces. IDs, time values, errors, and logging are separate units so later Frame and runtime work can depend on them without circular ownership.

---

### Task 1: Workspace bootstrap and quality baseline

**Files:**
- Create: `Cargo.toml`
- Create: `Cargo.lock`
- Create: `rust-toolchain.toml`
- Create: `.rustfmt.toml`
- Create: `.gitignore`
- Create: `crates/muxiva-types/Cargo.toml`
- Create: `crates/muxiva-types/src/lib.rs`
- Create: `crates/muxiva-core/Cargo.toml`
- Create: `crates/muxiva-core/src/lib.rs`
- Create: `crates/muxiva-examples/Cargo.toml`
- Create: `crates/muxiva-examples/src/lib.rs`

**Interfaces:**
- Consumes: Stage 1 package boundaries from `docs/design/01-product-and-technical-contract.md`.
- Produces: Cargo packages named `muxiva-types`, `muxiva-core`, and `muxiva-examples`; `muxiva_core` depends on `muxiva_types`; `muxiva_examples` depends on both public crates.

- [ ] **Step 1: Write a failing workspace metadata check**

Run before creating manifests:

```bash
cargo metadata --no-deps --format-version 1
```

Expected: FAIL because the repository has no root `Cargo.toml` (or because the required Rust toolchain is not yet available, which must be resolved before continuing).

- [ ] **Step 2: Create the minimal workspace and crate manifests**

Create the root manifest:

```toml
[workspace]
members = [
    "crates/muxiva-core",
    "crates/muxiva-examples",
    "crates/muxiva-types",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
rust-version = "1.85"
publish = false

[workspace.dependencies]
thiserror = "2.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt"] }
muxiva-types = { path = "crates/muxiva-types", version = "0.1.0" }
```

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

Create `.rustfmt.toml`:

```toml
edition = "2021"
newline_style = "Unix"
use_field_init_shorthand = true
```

Create `.gitignore` containing `/target/`, and define each crate with `edition.workspace = true`, `version.workspace = true`, `rust-version.workspace = true`, and `publish.workspace = true`. `muxiva-core` depends on `muxiva-types.workspace = true`; `muxiva-examples` depends on both crates by workspace path. Each initial `src/lib.rs` contains only `#![forbid(unsafe_code)]` and crate documentation. Stage 2 does not choose a license or assume a remote repository URL; release policy remains a pre-public-release decision from Stage 1.

- [ ] **Step 3: Verify workspace membership and dependency direction**

Run:

```bash
cargo metadata --no-deps --format-version 1
cargo check --workspace --all-targets
```

Expected: PASS; metadata lists exactly the three Stage 2 packages and no package uses Edition 2024.

- [ ] **Step 4: Run baseline formatting, linting, and tests**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Expected: PASS with no tests yet and no `unsafe` or async-runtime dependency.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml .rustfmt.toml .gitignore crates
git commit -m "build: bootstrap Rust workspace"
```

---

### Task 2: Strong IDs, timestamp, and sequence types

**Files:**
- Create: `crates/muxiva-types/src/id.rs`
- Create: `crates/muxiva-types/src/time.rs`
- Modify: `crates/muxiva-types/src/lib.rs`

**Interfaces:**
- Consumes: no production interfaces beyond Rust standard library.
- Produces: `SessionId`, `NodeId`, `StreamId`, and `TraceId` with `new`, `as_str`, `Display`, `FromStr`, and a shared `IdentifierError`; `Timestamp::from_nanos`, `Timestamp::as_nanos`, and `Timestamp::checked_add`; `SequenceId::new`, `SequenceId::get`, and `SequenceId::checked_next`.

- [ ] **Step 1: Write failing ID tests in `id.rs`**

Add tests that require distinct types, reject empty/whitespace/control-character IDs, preserve valid values, and round-trip through `Display`/`FromStr`:

```rust
#[test]
fn identifiers_validate_and_round_trip() {
    let node: NodeId = "asr.primary".parse().expect("valid node id");
    assert_eq!(node.as_str(), "asr.primary");
    assert_eq!(node.to_string(), "asr.primary");
    assert!(SessionId::new("").is_err());
    assert!(StreamId::new(" audio ").is_err());
    assert!(TraceId::new("trace\n1").is_err());
}
```

Also add a compile-fail documentation example on `NodeId` showing that a `SessionId` cannot be passed where `NodeId` is required.

- [ ] **Step 2: Run the ID tests and confirm failure**

Run:

```bash
cargo test -p muxiva-types id::tests -- --nocapture
```

Expected: FAIL because the ID types and constructors do not exist.

- [ ] **Step 3: Implement the ID types minimally**

Define `IdentifierError` and generate four non-interchangeable newtypes around `Box<str>`. Validation accepts 1 through 255 UTF-8 bytes and rejects leading/trailing whitespace plus ASCII control characters. Derive `Clone`, `Debug`, `Eq`, `Hash`, `Ord`, and their partial variants. Do not expose the wrapped field.

Use a private helper to avoid duplicating validation, while implementing the public constructors and traits separately for each concrete ID type.

- [ ] **Step 4: Write failing time tests in `time.rs`**

```rust
#[test]
fn timestamp_and_sequence_checked_arithmetic() {
    let timestamp = Timestamp::from_nanos(20);
    assert_eq!(timestamp.as_nanos(), 20);
    assert_eq!(timestamp.checked_add(22).unwrap().as_nanos(), 42);
    assert!(Timestamp::from_nanos(i64::MAX).checked_add(1).is_none());

    let sequence = SequenceId::new(7);
    assert_eq!(sequence.get(), 7);
    assert_eq!(sequence.checked_next().unwrap().get(), 8);
    assert!(SequenceId::new(u64::MAX).checked_next().is_none());
}
```

- [ ] **Step 5: Run the time tests and confirm failure**

Run:

```bash
cargo test -p muxiva-types time::tests -- --nocapture
```

Expected: FAIL because `Timestamp` and `SequenceId` do not exist.

- [ ] **Step 6: Implement time types and public exports**

Implement `Timestamp(i64)` as signed nanoseconds so media-relative values can represent pre-roll, and `SequenceId(u64)` as an ordered counter. Derive copyable ordering/hash traits and use checked arithmetic only. Export all ID/time types from `muxiva-types/src/lib.rs` without exposing module internals.

- [ ] **Step 7: Verify the package**

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --all-targets
cargo test -p muxiva-types --doc
```

Expected: PASS, including the compile-fail type-separation documentation test.

- [ ] **Step 8: Commit**

```bash
git add crates/muxiva-types/src
git commit -m "feat(types): add strong identifiers and time values"
```

---

### Task 3: Structured contextual errors

**Files:**
- Create: `crates/muxiva-types/src/error.rs`
- Modify: `crates/muxiva-types/src/lib.rs`
- Modify: `crates/muxiva-types/Cargo.toml`

**Interfaces:**
- Consumes: `NodeId` from Task 2.
- Produces: `ErrorCategory`, `ErrorContext`, `MuxivaError`, and `pub type Result<T> = std::result::Result<T, MuxivaError>`; builder methods `with_node`, `with_phase`, and `with_context`; accessors `category`, `code`, `message`, and `contexts`.

- [ ] **Step 1: Add failing error tests**

```rust
#[test]
fn error_preserves_category_code_and_context() {
    let node = NodeId::new("mock-asr").unwrap();
    let error = MuxivaError::new(ErrorCategory::Configuration, "MUXIVA-CFG-001", "missing model")
        .with_node(node.clone())
        .with_phase("prepare")
        .with_context("config_key", "model");

    assert_eq!(error.category(), ErrorCategory::Configuration);
    assert_eq!(error.code(), "MUXIVA-CFG-001");
    assert_eq!(error.message(), "missing model");
    assert!(error.to_string().contains("MUXIVA-CFG-001"));
    assert_eq!(error.contexts().len(), 3);
    assert_eq!(error.contexts()[0], ErrorContext::Node(node));
}

#[test]
fn error_rejects_invalid_stable_code() {
    assert!(MuxivaError::try_new(ErrorCategory::Internal, "temporary code", "failure").is_err());
}
```

- [ ] **Step 2: Run the focused tests and confirm failure**

Run:

```bash
cargo test -p muxiva-types error::tests -- --nocapture
```

Expected: FAIL because the error module is absent.

- [ ] **Step 3: Implement the minimal error contract**

Add `thiserror.workspace = true`. Define stable categories `Configuration`, `Validation`, `Lifecycle`, `Cancelled`, `External`, and `Internal`. Define structured contexts as:

```rust
pub enum ErrorContext {
    Session(SessionId),
    Node(NodeId),
    Stream(StreamId),
    Phase(Box<str>),
    Detail { key: Box<str>, value: Box<str> },
}
```

`MuxivaError` stores category, validated stable code, message, contexts, and an optional boxed source error. Stable codes accept uppercase ASCII letters, digits, and hyphens, begin with `MUXIVA-`, and contain 6 through 64 bytes. `Display` renders `[CODE] message`; it does not automatically print all contexts, preventing accidental sensitive logging. `with_source` accepts an error that is `Send + Sync + 'static`.

- [ ] **Step 4: Export and verify errors**

Export the four public names from `lib.rs`, then run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --all-targets
```

Expected: PASS; invalid codes fail construction, structured context order is preserved, and source chaining is available through `std::error::Error::source`.

- [ ] **Step 5: Commit**

```bash
git add crates/muxiva-types
git commit -m "feat(types): add contextual error contract"
```

---

### Task 4: Replaceable logging with idempotent tracing default

**Files:**
- Create: `crates/muxiva-core/src/logging.rs`
- Modify: `crates/muxiva-core/src/lib.rs`
- Modify: `crates/muxiva-core/Cargo.toml`

**Interfaces:**
- Consumes: `NodeId` and `SessionId` from `muxiva-types`.
- Produces: `LogLevel`, `LogRecord`, object-safe `LogSink`, `TracingLogSink`, and idempotent `init_default_logging() -> muxiva_types::Result<()>`.

- [ ] **Step 1: Write failing replaceability and record tests**

Define a test-only collecting sink and assert the public trait is usable without `tracing`:

```rust
#[test]
fn custom_sink_receives_structured_record() {
    let sink = CollectSink::default();
    let record = LogRecord::new(LogLevel::Info, "runtime.started")
        .with_session(SessionId::new("session-1").unwrap())
        .with_field("worker_count", "2");
    sink.emit(&record);
    assert_eq!(sink.records.lock().unwrap().as_slice(), &[record]);
}
```

Also test that reserved/sensitive field names `payload`, `authorization`, and `private_extension` are rejected with error code `MUXIVA-LOG-001`.

- [ ] **Step 2: Run focused tests and confirm failure**

Run:

```bash
cargo test -p muxiva-core logging::tests -- --nocapture
```

Expected: FAIL because no logging module exists.

- [ ] **Step 3: Implement the logging abstraction**

Add workspace dependencies on `tracing` and `tracing-subscriber`. `LogRecord` contains level, stable event name, optional session/node identity, and an ordered list of string fields. The object-safe trait is:

```rust
pub trait LogSink: Send + Sync {
    fn emit(&self, record: &LogRecord);
}
```

`TracingLogSink` maps each `LogLevel` to the matching `tracing` macro and emits event, session, node, plus a debug representation of validated fields. It never receives a raw Frame or media payload.

- [ ] **Step 4: Write the failing repeated-initialization test**

```rust
#[test]
fn default_logging_initialization_is_idempotent() {
    init_default_logging().expect("first initialization");
    init_default_logging().expect("second initialization");
}
```

Run:

```bash
cargo test -p muxiva-core logging::tests::default_logging_initialization_is_idempotent -- --exact
```

Expected: FAIL until default initialization exists.

- [ ] **Step 5: Implement idempotent default initialization**

Use `std::sync::OnceLock` to attempt `tracing_subscriber::fmt().try_init()` at most once. Treat an already-installed global subscriber as a usable logging environment and return `Ok(())`; do not replace it or panic. Repeated and concurrent calls return `Ok(())`.

- [ ] **Step 6: Verify logging and workspace regressions**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Expected: PASS, including custom sink, sensitive field rejection, and repeated initialization tests.

- [ ] **Step 7: Commit**

```bash
git add crates/muxiva-core
git commit -m "feat(core): add replaceable structured logging"
```

---

### Task 5: Runnable hello example and Linux CI

**Files:**
- Modify: `crates/muxiva-examples/src/lib.rs`
- Create: `crates/muxiva-examples/src/bin/hello.rs`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `SessionId`, `LogRecord`, `LogLevel`, `TracingLogSink`, `LogSink`, and `init_default_logging`.
- Produces: `muxiva_examples::hello_message(&SessionId) -> String` and the `hello` binary.

- [ ] **Step 1: Write a failing example-library test**

```rust
#[test]
fn hello_message_contains_typed_session_id() {
    let session = SessionId::new("hello-session").unwrap();
    assert_eq!(hello_message(&session), "Muxiva runtime ready: hello-session");
}
```

- [ ] **Step 2: Run the focused test and confirm failure**

Run:

```bash
cargo test -p muxiva-examples --lib -- --nocapture
```

Expected: FAIL because `hello_message` does not exist.

- [ ] **Step 3: Implement the library function and binary**

Implement `hello_message` as a pure formatter. In `hello.rs`, construct `SessionId("hello-session")`, initialize logging twice to demonstrate idempotence, emit a `runtime.ready` record through `TracingLogSink`, and print exactly the string returned by `hello_message`. Return `muxiva_types::Result<()>` from `main`.

- [ ] **Step 4: Verify the executable output**

Run:

```bash
cargo run -p muxiva-examples --bin hello
```

Expected stdout contains exactly one line:

```text
Muxiva runtime ready: hello-session
```

Structured tracing output may additionally appear on stderr.

- [ ] **Step 5: Add the Linux CI workflow**

Create `.github/workflows/ci.yml` triggered on pushes and pull requests. Use `ubuntu-latest`, `actions/checkout@v4`, and `dtolnay/rust-toolchain@stable` with `rustfmt,clippy`. Run these separate named steps:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p muxiva-examples --bin hello
```

- [ ] **Step 6: Run all local quality gates**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p muxiva-examples --bin hello
cargo tree --workspace
```

Expected: all commands pass; `cargo tree` contains no Tokio, async runtime, FFI, or media crate.

- [ ] **Step 7: Commit**

```bash
git add crates/muxiva-examples .github/workflows/ci.yml
git commit -m "ci: validate hello foundation workspace"
```

---

### Task 6: Stage report and final contract audit

**Files:**
- Create: `docs/pre_release_notes/02-rust-foundation.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: verified commands and public APIs from Tasks 1 through 5.
- Produces: the Stage 2 report required by the foundation contract and a README status link.

- [ ] **Step 1: Capture objective verification evidence**

Run and record the exact toolchain versions and test totals:

```bash
rustc --version
cargo --version
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p muxiva-examples --bin hello
cargo tree --workspace
```

Expected: every command exits zero, the hello output matches Task 5, and the dependency tree respects Stage 2 boundaries.

- [ ] **Step 2: Write the Stage 2 report**

Document the file list, public API list, single-threaded/no-runtime thread model, owned immutable foundation values, test commands with observed totals, dependency audit, risks, and recommendation for Stage 3. State explicitly that no graph execution, Frame, Tokio, FFI, Python, C++, TypeScript, RTC, or FFmpeg code was added.

- [ ] **Step 3: Update README status without claiming Stage 2 acceptance**

Add a link to `docs/pre_release_notes/02-rust-foundation.md` and state that Stage 2 is implemented and awaiting acceptance. Do not describe Stage 3 as started.

- [ ] **Step 4: Run document and repository checks**

Run:

```bash
if rg -n '[T]ODO|[T]BD|[F]IXME' README.md docs; then exit 1; fi
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Expected: PASS with no placeholder or whitespace findings and all Rust gates green.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/pre_release_notes/02-rust-foundation.md
git commit -m "docs: report Stage 2 Rust foundation"
```

- [ ] **Step 6: Stop at the Stage 2 gate**

Present the file list, APIs, ownership/thread model, exact verification results, risks, and commit list for user acceptance. Do not create Stage 3 types or plans until the user explicitly accepts Stage 2.
