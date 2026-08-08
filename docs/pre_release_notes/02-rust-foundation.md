# Muxiva Pre-release Notes: Stage 2 Rust Foundation

Date: **2026-08-01**

Status: **Closed for sequencing by maintainer direction; review findings deferred.**

## Scope and recommendation

Stage 2 establishes the Rust Edition 2021 workspace, public foundation values,
contextual errors, replaceable structured logging, a runnable hello example,
and a Linux CI workflow. The repository is at the Stage 2 gate; Stage 3 has
not started.

The later roadmap inputs now extend through Stage 11. They are not reproduced,
planned in detail, or implemented by this stage report.

Recommendation: accept the Stage 2 gate only after reviewing the interfaces,
risks, and verification evidence below. On acceptance, begin only the next
explicitly approved stage.

## Delivered files

- Workspace and quality baseline: `Cargo.toml`, `Cargo.lock`,
  `rust-toolchain.toml`, `.rustfmt.toml`, `.gitignore`, and
  `.github/workflows/ci.yml`.
- `crates/muxiva-types`: `Cargo.toml`, `src/lib.rs`, `src/id.rs`, `src/time.rs`,
  and `src/error.rs`.
- `crates/muxiva-core`: `Cargo.toml`, `src/lib.rs`, and `src/logging.rs`.
- `crates/muxiva-examples`: `Cargo.toml`, `src/lib.rs`, and
  `src/bin/hello.rs`.
- Documentation: this report, the Stage 2 implementation plan, and the README
  status link.

## Public API contract

`muxiva-types` is the dependency-light owner of immutable, owned foundation
values:

- Distinct `NodeId`, `SessionId`, `StreamId`, and `TraceId` newtypes, each with
  fallible `new`, `as_str`, `Display`, and `FromStr`; shared `IdentifierError`
  exposes `Empty`, `TooLong`, `LeadingOrTrailingWhitespace`, and
  `ContainsControlCharacter` validation failures.
- `Timestamp::from_nanos`, `Timestamp::as_nanos`, and
  `Timestamp::checked_add`; `SequenceId::new`, `SequenceId::get`, and
  `SequenceId::checked_next`.
- `ErrorCategory` variants `Configuration`, `Validation`, `Lifecycle`,
  `Cancelled`, `External`, and `Internal`; `ErrorContext` variants `Session`,
  `Node`, `Stream`, `Phase`, and `Detail`; plus `ErrorCodeError`, `MuxivaError`,
  and `Result<T>`. `MuxivaError::new` validates its error code and panics for an
  invalid code; `MuxivaError::try_new` is the fallible constructor. Builders are
  `with_node`, `with_phase`, `with_context`, and `with_source`; accessors are
  `category`, `code`, `message`, and `contexts`.

`muxiva-core::logging` owns runtime-facing logging services:

- `LogLevel` variants `Error`, `Warn`, `Info`, `Debug`, and `Trace`;
  `LogRecord::new` is fallible and validates a stable lowercase ASCII dotted
  event name, returning `MUXIVA-LOG-002` on rejection.
- `LogRecord::with_session`, `with_node`, `level`, `event_name`, `session`,
  `node`, and `fields`.
- `LogRecord::with_field` is fallible and rejects reserved field names with
  `MUXIVA-LOG-001`.
- Object-safe `LogSink: Send + Sync` with required method
  `fn emit(&self, record: &LogRecord)`, `TracingLogSink`, and idempotent
  `init_default_logging() -> muxiva_types::Result<()>`.

`muxiva-examples` exposes `hello_message(&SessionId) -> String`; its `hello`
binary initializes default logging twice, emits a structured readiness record,
and prints the typed-session readiness message.

## Ownership and thread model

Foundation values own their data (`Box<str>`, `Vec`, and owned error sources)
and expose immutable borrows or copyable scalar values. Identifier types are
not aliases and cannot be interchanged. No public API exposes a borrowed
buffer, an ownership-transfer protocol, or a release-thread requirement.

This stage adds no runtime, executor, task spawning, worker threads, or async
API. Its runtime model is single-threaded/no-runtime; `LogSink` has `Send +
Sync` bounds so callers may provide a thread-safe sink when a later approved
stage introduces concurrency. The default initializer uses `OnceLock` only to
make tracing setup idempotent, not to create threads.

## Boundary and dependency audit

The workspace contains exactly `muxiva-types`, `muxiva-core`, and
`muxiva-examples`. Dependency direction is one-way:

```text
muxiva-examples -> muxiva-core -> muxiva-types
                  └--------> muxiva-types
```

`muxiva-types` depends only on `thiserror`; `muxiva-core` adds `tracing` and
`tracing-subscriber`; `muxiva-examples` depends on the two local public crates.
The fresh `cargo tree --workspace` output below shows the complete resolved
tree. It contains no Tokio, async runtime, graph runtime, media, RTC, or FFmpeg
dependency.

No graph execution, `Frame`, Tokio, FFI, Python, C++, TypeScript, RTC, or
FFmpeg code was added. Each crate forbids unsafe code at its root, and the
resolved dependency tree contains no async runtime.

## Fresh verification evidence

All commands in this section were run in
`<repo>` on
2026-08-01 and exited with status 0. Outputs are recorded exactly as observed;
an empty block means the command produced no output.

### Toolchain and required Stage 2 commands

```text
$ rustc --version
rustc 1.97.1 (8bab26f4f 2026-07-14)

$ cargo --version
cargo 1.97.1 (c980f4866 2026-06-30)

$ cargo fmt --all --check

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
```

```text
$ cargo test --workspace --all-targets
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
     Running unittests src/lib.rs (target/debug/deps/muxiva_core-fbde4fafb6d7c07a)

running 7 tests
test logging::tests::rejects_unstable_event_names ... ok
test logging::tests::record_preserves_identity_and_field_insertion_order ... ok
test logging::tests::rejects_authorization_field_to_prevent_sensitive_logging ... ok
test logging::tests::rejects_payload_field_to_prevent_sensitive_logging ... ok
test logging::tests::rejects_private_extension_field_to_prevent_sensitive_logging ... ok
test logging::tests::custom_sink_receives_structured_record ... ok
test logging::tests::default_logging_initialization_is_idempotent ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/muxiva_examples-005393afdccd6d74)

running 1 test
test tests::hello_message_contains_typed_session_id ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/bin/hello.rs (target/debug/deps/hello-d01adf6c95358e8e)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/muxiva_types-018e73d09abc7686)

running 7 tests
test id::tests::identifiers_validate_and_round_trip ... ok
test error::tests::error_exposes_its_source ... ok
test error::tests::error_rejects_invalid_stable_code ... ok
test error::tests::error_preserves_category_code_and_context ... ok
test time::tests::timestamp_and_sequence_checked_arithmetic ... ok
test error::tests::error_code_error_is_reachable_from_the_crate_root ... ok
test error::tests::error_display_omits_sensitive_context_values ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Observed total: **15 passed, 0 failed** across three library test binaries and
one zero-test example binary.

```text
$ cargo run -p muxiva-examples --bin hello
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.01s
     Running `target/debug/hello`
Muxiva runtime ready: hello-session
2026-07-31T16:24:31.915174Z  INFO muxiva_core::logging: Muxiva event event=runtime.ready session=Some(SessionId("hello-session")) node=None fields=[("example", "hello")]
```

```text
$ cargo tree --workspace
muxiva-core v0.1.0 (<repo>/crates/muxiva-core)
├── tracing v0.1.44
│   ├── pin-project-lite v0.2.17
│   ├── tracing-attributes v0.1.31 (proc-macro)
│   │   ├── proc-macro2 v1.0.107
│   │   │   └── unicode-ident v1.0.24
│   │   ├── quote v1.0.47
│   │   │   └── proc-macro2 v1.0.107 (*)
│   │   └── syn v2.0.119
│   │       ├── proc-macro2 v1.0.107 (*)
│   │       ├── quote v1.0.47 (*)
│   │       └── unicode-ident v1.0.24
│   └── tracing-core v0.1.36
│       └── once_cell v1.21.4
├── tracing-subscriber v0.3.23
│   ├── nu-ansi-term v0.50.3
│   ├── sharded-slab v0.1.7
│   │   └── lazy_static v1.5.0
│   ├── smallvec v1.15.2
│   ├── thread_local v1.1.10
│   │   └── cfg-if v1.0.4
│   ├── tracing-core v0.1.36 (*)
│   └── tracing-log v0.2.0
│       ├── log v0.4.33
│       ├── once_cell v1.21.4
│       └── tracing-core v0.1.36 (*)
└── muxiva-types v0.1.0 (<repo>/crates/muxiva-types)
    └── thiserror v2.0.19
        └── thiserror-impl v2.0.19 (proc-macro)
            ├── proc-macro2 v1.0.107 (*)
            ├── quote v1.0.47 (*)
            └── syn v3.0.3
                ├── proc-macro2 v1.0.107 (*)
                ├── quote v1.0.47 (*)
                └── unicode-ident v1.0.24

muxiva-examples v0.1.0 (<repo>/crates/muxiva-examples)
├── muxiva-core v0.1.0 (<repo>/crates/muxiva-core) (*)
└── muxiva-types v0.1.0 (<repo>/crates/muxiva-types) (*)

muxiva-types v0.1.0 (<repo>/crates/muxiva-types) (*)
```

### Fresh document and repository checks

These checks were run after this report was written. Each command exited with
status 0. The placeholder scan, diff check, and formatting check produced no
output; Clippy finished without diagnostics, and the test gate repeated the
observed total of **15 passed, 0 failed**.

```text
$ if rg -n '[T]ODO|[T]BD|[F]IXME' README.md docs; then exit 1; fi

$ git diff --check

$ cargo fmt --all --check

$ cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.13s

$ cargo test --workspace --all-targets
muxiva-core: 7 passed; 0 failed
muxiva-examples library: 1 passed; 0 failed
hello binary: 0 passed; 0 failed
muxiva-types: 7 passed; 0 failed
observed total: 15 passed; 0 failed
```

## Known risks and deferred decisions

- The foundation has no graph or frame semantics yet, so its suitability for
  graph scheduling, backpressure, and lifecycle ordering remains unvalidated.
- Logging intentionally rejects only three reserved names and stable event-name
  structure; a later security and observability policy may require a broader
  sensitive-data taxonomy.
- `init_default_logging` intentionally ignores an existing tracing subscriber;
  later embedding requirements may need a caller-owned subscriber strategy.
- The test suite validates public foundation behavior but does not establish
  cross-platform, performance, concurrency, FFI, media, or integration claims.
- License, governance, security policy, release signing, and public publishing
  decisions remain deferred from the Stage 1 contract.

## Commit evidence

Stage 2 implementation commits before this documentation commit:

```text
4539c78 docs: plan Stage 2 Rust foundation
c87bc38 chore: ignore local worktrees
61d4ed3 build: bootstrap Rust workspace
d4f9257 build: commit workspace lockfile
740929b feat(types): add strong identifiers and time values
7300bbc feat(types): add contextual error contract
6ea7ef1 fix(types): expose error code validation type
7c31975 feat(core): add replaceable structured logging
a197b5b fix(core): validate logging event names
213ea21 ci: validate hello foundation workspace
```

The final documentation commit is recorded after the fresh repository checks.

## Self-review and concerns

Self-review found the documented API signatures aligned with the code at the
current `HEAD`, including the fallible `LogRecord::new` and `with_field`
builders plus validated event names. The dependency tree and source scan match
the declared Stage 2 boundaries. No production-code defect was observed, so no
scope-expanding change was made.

This report makes no claim of CI execution, performance measurement, or
later-stage readiness.

## Deferred final-review findings

On 2026-08-01, the maintainer directed development to continue to Stage 3
without resolving the final Stage 2 review findings. This is a sequencing
decision, not a claim that the findings passed review.

The deferred contract findings are:

- the default `TracingLogSink` can emit arbitrary field values and therefore
  does not yet enforce the Stage 1 default-log privacy boundary; and
- `ErrorContext::Session` and `ErrorContext::Stream` cannot yet be attached
  through public `MuxivaError` builder methods.

The deferred verification and documentation findings cover tracing-output
capture, concurrent and pre-installed subscriber initialization, identifier
length boundaries, timestamp clock-domain wording, event-name grammar wording,
the stale fallible logging example in the implementation plan, and labeling a
summarized test-result block as summarized rather than literal output.

These findings must remain visible in later reviews and must be resolved before
Muxiva claims the Stage 2 foundation is quality-clean or publishes a release.
