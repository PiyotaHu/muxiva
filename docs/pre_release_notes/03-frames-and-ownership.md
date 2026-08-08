# Muxiva Pre-release Notes: Stage 3 Frames and Ownership

Date: **2026-08-01**

Status: **Implemented and awaiting acceptance; review findings deferred.**

## Scope and stop gate

Stage 3 delivers immutable, owned Frames in six variants; checked audio and
video layouts; versioned metadata and extensions; lineage-preserving
derivation; privacy-aware borrowed views; and a construction-only example.
This is a data-model and ownership stage. It adds no graph, runtime, queue,
lifecycle hook, binding, media integration, or transport.

Live `Frame`, `FrameHeader`, `FrameBuffer`, `Value`, metadata, extension, and
lineage values have no serde implementation and no JSON representation. Copy
and Retain/Release remain design-only future FFI modes. Stage 4 has not
started, and this report does not authorize it.

## Delivered files

- Contract and plan:
  `docs/design/03-frame-and-ownership-contract.md` and
  `docs/superpowers/plans/2026-08-01-stage-3-frames-and-ownership.md`.
- Identity and scalar values: `crates/muxiva-types/src/id.rs`, `time.rs`,
  `schema.rs`, and `value.rs`.
- Ownership and metadata: `crates/muxiva-types/src/frame_buffer.rs`,
  `extension.rs`, and `lineage.rs`.
- Frame implementation: `crates/muxiva-types/src/frame/mod.rs`, `header.rs`,
  `audio.rs`, `video.rs`, and `message.rs`, exported by `src/lib.rs`.
- Public behavior tests: `crates/muxiva-types/tests/frame_contract.rs`,
  `frame_derivation.rs`, and `frame_concurrency.rs`, plus colocated unit and
  compile-fail documentation tests.
- Consumer example: `crates/muxiva-examples/src/bin/frames.rs`.
- Status documentation: this report and the README Stage 3 status/link.

## Public API delivered

The foundation adds nominal `FrameId`, `EdgeId`, `ClockDomainId`, and
`ProducerId` values with the existing validated identifier contract. It adds
validated `SchemaVersion` and `NamespacedName`, finite-number `Value`, ordered
immutable `ValueMap` and `Metadata`, and the immutable Arc-backed
`FrameBuffer`.

Extension APIs comprise `ExtensionVisibility`, `ExtensionProducer`,
`Extension`, and `Extensions`; all extensions remain available to receivers,
while `public_iter` filters private records. Lineage APIs comprise
`TransformOrigin`, `LineageEntry`, and read-only `Lineage`.

Clock/header APIs comprise `ClockKind`, `ClockDomain`, `FrameType`, and
`FrameHeader`, including `FrameHeader::compare_timestamp`. Payload APIs are:

- PCM `AudioData`, `AudioLayout`, and six `PcmSampleFormat` values;
- `VideoData`, `VideoLayout`, immutable `VideoPlane`, and `PixelFormat` for
  RGBA8 and YUV420P;
- owned `TextData`, optional validated `MediaType` plus `ByteData`, and
  namespaced `SignalData` and `EventData` values.

`FramePayload` and `Frame` each have exactly `Audio`, `Video`, `Text`, `Byte`,
`Signal`, and `Event` variants. `Frame::new` validates header/payload type
agreement; immutable accessors and `ensure_type` provide dispatch and type
gating. `FrameDerivation` and `Frame::derive` create a child while preserving
the parent, common identity context, metadata, extensions, and payload by
default, appending exactly one lineage record. Callers may explicitly replace
metadata, extensions, or payload. `PublicFrameView`,
`PublicFrameHeaderView`, and `LogSafeFrameView` provide borrowed diagnostic
surfaces without creating a serialization format.

## Validation and stable errors

All failures below use `ErrorCategory::Validation`.

| Code | Rejected condition |
| --- | --- |
| `MUXIVA-FRM-SCHEMA-VERSION` | zero schema version |
| `MUXIVA-FRM-NAMESPACE` | invalid extension, signal, or topic namespace |
| `MUXIVA-FRM-VALUE-NUMBER` | non-finite floating point value |
| `MUXIVA-FRM-VALUE-KEY` | invalid or duplicate Value-map/Metadata key |
| `MUXIVA-FRM-EXTENSION-DUPLICATE` | duplicate extension key and schema version |
| `MUXIVA-FRM-LINEAGE-ORIGIN` | lineage has neither Node nor Edge source |
| `MUXIVA-FRM-LINEAGE-REASON` | empty, oversized, or control-bearing reason |
| `MUXIVA-FRM-LINEAGE-CYCLE` | new header names itself as a parent |
| `MUXIVA-FRM-CLOCK-DOMAIN` | timestamp ordering crosses complete clock domains |
| `MUXIVA-FRM-TYPE-MISMATCH` | header, payload, or expected FrameType differs |
| `MUXIVA-FRM-AUDIO-RATE` | sample rate outside 1..=768,000 |
| `MUXIVA-FRM-AUDIO-CHANNELS` | channel count outside 1..=1,024 |
| `MUXIVA-FRM-AUDIO-SAMPLES` | zero samples per channel |
| `MUXIVA-FRM-AUDIO-LENGTH` | payload length differs from checked exact length |
| `MUXIVA-FRM-AUDIO-PLANE` | plane index is absent from the audio layout |
| `MUXIVA-FRM-VIDEO-DIMENSIONS` | zero dimensions or odd YUV420P dimensions |
| `MUXIVA-FRM-VIDEO-STRIDE` | stride is smaller than its row bytes |
| `MUXIVA-FRM-VIDEO-LENGTH` | payload length differs from checked plane total |
| `MUXIVA-FRM-VIDEO-PLANE` | descriptor is not borrowed from that VideoData layout |
| `MUXIVA-FRM-ARITHMETIC` | checked size, offset, or duration arithmetic overflow |
| `MUXIVA-FRM-TEXT-UTF8` | invalid UTF-8 text bytes |
| `MUXIVA-FRM-MEDIA-TYPE` | invalid optional media type |
| `MUXIVA-FRM-DERIVATION-ID` | derivation reuses its direct parent's FrameId |

Media length, stride, offset, plane, and duration calculations use checked
integer arithmetic before conversion or slicing. Errors attach scalar
diagnostics only, never payload or private-extension content.

## Ownership, threads, and diagnostic views

`FrameBuffer` owns `Arc<[u8]>` and exposes only immutable bytes. Clone shares
the allocation; the last clone releases it on its dropping thread. `Frame`,
buffers, and their contained values are `Send + Sync`, and fresh tests read a
shared Frame concurrently without locks. Stage 3 does not create threads in
production code, queues, workers, or an async runtime.

Future Copy mode requires an Adapter to copy SDK bytes before a callback
returns. Future Retain/Release mode requires an explicit SDK lifetime promise,
exactly-once release after the final Core reference, and an Adapter-owned post
back to the required release thread when affinity exists. Neither mode has an
FFI implementation, pointer, callback, handle, or release queue in Stage 3.

Receivers can inspect all extensions through the full immutable header.
`public_view` excludes private extensions but otherwise exposes immutable
header data. `log_safe_view` exposes identity and scalar counts, including a
payload byte length only where cheaply known. Default `Debug` output for
Frame, FrameHeader, FrameBuffer, and Extension omits values and payloads and
redacts private extension keys. Privacy here is a diagnostic boundary, not
encryption or authorization.

## Timestamp breaking correction and migration

Stage 3 removes `Ord` and `PartialOrd` from `Timestamp`. This is an intentional
pre-1.0 breaking correction: a signed nanosecond scalar has no valid ordering
without its clock domain. Bare `<`/`>`, sorting, `cmp`, `min`, and `max` over
timestamps are affected. Migrate ordering to
`FrameHeader::compare_timestamp`, which requires equality of the entire
`ClockDomain` before comparing signed nanoseconds.

Fresh evidence includes a compile-fail documentation test proving raw
timestamp ordering does not compile, an integration test covering Less,
Greater, and Equal in one domain, and a test proving equal
`ClockKind::MediaRelative` values with different domain IDs return
`MUXIVA-FRM-CLOCK-DOMAIN`. The Stage 2 timestamp clock-domain wording and raw
ordering finding is resolved by this stage.

## Example evidence

The example constructs 48 kHz mono interleaved I16LE audio with 480 samples
and 960 owned bytes, a public unknown extension, a private extension, and an
empty-lineage `frame-1`. It derives `frame-2` through Node `normalize` and Edge
`capture-to-normalize` with reason `normalize-volume`, using a separately
allocated 960-byte child buffer. Assertions preserve the parent and unknown
extension. Fresh execution produced exactly:

```text
Muxiva derived frame: frame-2 Audio lineage=1
```

## Fresh verification evidence

All commands in this section ran in the Stage 3 worktree on 2026-08-01 and
exited successfully. No remote CI result or performance measurement is
claimed.

```text
$ rustc --version
rustc 1.97.1 (8bab26f4f 2026-07-14)

$ cargo --version
cargo 1.97.1 (c980f4866 2026-06-30)
```

Focused acceptance:

```text
$ cargo test -p muxiva-types --all-targets
muxiva-types unit: 26 passed; frame_concurrency: 3 passed;
frame_contract: 18 passed; frame_derivation: 6 passed
observed total: 53 passed; 0 failed

$ cargo test -p muxiva-types --doc
4 passed; 0 failed (all four are compile-fail tests)

$ cargo test -p muxiva-examples --all-targets
muxiva-examples library: 1 passed; frames binary: 0; hello binary: 0
observed total: 1 passed; 0 failed

$ cargo run -p muxiva-examples --bin frames
Muxiva derived frame: frame-2 Audio lineage=1
```

Complete local quality gates:

```text
$ cargo fmt --all --check
no output

$ cargo clippy --workspace --all-targets -- -D warnings
Finished successfully with no diagnostics

$ cargo test --workspace --all-targets
muxiva-core: 7 passed; muxiva-examples: 1 passed; muxiva-types: 53 passed
observed total: 61 passed; 0 failed

$ cargo test --workspace --doc
muxiva-core: 0; muxiva-examples: 0; muxiva-types: 4 passed
observed total: 4 passed; 0 failed
```

The forbidden-capability scan found no unsafe allowance/block, Tokio, serde
derive/use, C ABI, unmangled export, GraphBuilder/GraphRunner, exact Node/Edge/
Graph declaration, queue, media/RTC/FFmpeg, Python, or Node-API implementation.
The declaration expression used word boundaries so allowed identifiers such
as `NodeId` and `EdgeId` could not create false positives. A second exact-name
scan found no runtime or routing declarations. The public immutability scan
found no mutable borrow/pointer API, owned byte-vector return, Arc storage
return, or public mutable/storage field. `git diff --check` also passed.

## Dependency audit

No dependency changed in Stage 3. Fresh `cargo tree --workspace` resolved:

```text
muxiva-core v0.1.0
├── tracing v0.1.44
│   ├── pin-project-lite v0.2.17
│   ├── tracing-attributes v0.1.31
│   │   ├── proc-macro2 v1.0.107
│   │   ├── quote v1.0.47
│   │   ├── syn v2.0.119
│   │   └── unicode-ident v1.0.24
│   └── tracing-core v0.1.36
│       └── once_cell v1.21.4
├── tracing-subscriber v0.3.23
│   ├── nu-ansi-term v0.50.3
│   ├── sharded-slab v0.1.7
│   │   └── lazy_static v1.5.0
│   ├── smallvec v1.15.2
│   ├── thread_local v1.1.10
│   │   └── cfg-if v1.0.4
│   ├── tracing-core v0.1.36
│   └── tracing-log v0.2.0
│       ├── log v0.4.33
│       └── once_cell v1.21.4
└── muxiva-types v0.1.0
    └── thiserror v2.0.19
        └── thiserror-impl v2.0.19
            ├── proc-macro2 v1.0.107
            ├── quote v1.0.47
            ├── syn v3.0.3
            └── unicode-ident v1.0.24

muxiva-examples v0.1.0
├── muxiva-core v0.1.0
└── muxiva-types v0.1.0
```

The tree contains no Tokio, serde, async runtime, FFI, media, RTC, or FFmpeg
package. `muxiva-types` remains dependent only on `thiserror`.

## Stage 3 review debt

The following non-blocking findings are recorded as technical debt and do not
block stage sequencing:

- no test covers equal `ClockDomainId` with different `ClockKind` values;
- valid padded video stride layouts lack positive-case coverage;
- `PublicFrameHeaderView` lacks the `frame_type` accessor required by the
  Stage 3 design;
- `LogSafeFrameView::payload_byte_len` coverage is incomplete for Audio,
  Video, Signal, and Event payloads;
- compile-fail coverage does not explicitly guard future `AsMut` or
  `DerefMut` additions; and
- the Task 9 frames example has no automated regression test for its exact
  one-line output; acceptance currently relies on fresh `cargo run` evidence.

These findings are not represented as passing coverage. They must be resolved
before Stage 3 is described as quality-clean.

## Stage 2 debt carried forward

By maintainer direction, all Stage 2 findings other than the timestamp item
remain deferred. This is a sequencing decision, not a claim that they passed
review:

- the default `TracingLogSink` can emit arbitrary field values and therefore
  does not yet enforce the Stage 1 default-log privacy boundary;
- `ErrorContext::Session` and `ErrorContext::Stream` cannot yet be attached
  through public `MuxivaError` builder methods; and
- verification/documentation coverage for tracing-output capture, concurrent
  and pre-installed subscriber initialization, identifier length boundaries,
  event-name grammar wording, the stale fallible logging example in the Stage
  2 implementation plan, and labeling a summarized test-result block as
  summarized rather than literal output remains deferred.

The former Stage 2 timestamp clock-domain wording/ordering issue is the sole
resolved item, with correction and migration evidence documented above.
License, governance, security policy, release signing, and public publishing
decisions also remain deferred from Stage 1.

## Commit evidence

Stage 3 commits before this report commit are:

```text
a6532f6 docs: design Stage 3 frame contract
1754ce1 docs: correct Stage 3 frame invariants
6160339 feat(types): add frame identity and schema values
163e83d feat(types): add immutable frame buffers and values
e4ec6cc feat(types): add frame extensions and lineage
06cfeaa feat(types): validate frame headers and PCM audio
8e39b96 feat(types): validate immutable video layouts
395b523 feat(types): assemble six immutable frame variants
1d22a11 feat(types): derive frames with safe diagnostic views
68153f6 test(types): verify frame ownership and concurrent reads
a5f29b6 feat(examples): construct and derive immutable frames
```

The final documentation commit is recorded after the report and README checks.

## Recommendation and concerns

The observed Stage 3 implementation and required local gates are green. The
listed Stage 2 and Stage 3 technical debt remains visible and unclaimed. No
remote CI, performance, FFI lifetime, cross-platform, graph execution, or
production runtime behavior was tested. Review this report for Stage 3
acceptance, then stop; Stage 4 has not started.
