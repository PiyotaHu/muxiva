# Muxiva Stage 3 Frames and Ownership Implementation Plan

> **Execution rule:** Implement one checklist item at a time with a failing
> test first. Do not start Stage 4 work while executing this plan.

**Goal:** Implement the immutable six-variant Frame family, checked media
layouts, Arc-backed read-only buffers, privacy-aware extensions and views,
lineage-preserving derivation, and a construction-only example.

**Architecture:** `muxiva-types` remains the dependency-light owner. Small
modules own IDs/schema, buffers, values, extensions, lineage, header, audio,
video, and non-media messages. `Frame` assembles validated parts and is the
only future transport unit. `muxiva-examples` proves consumer ergonomics without
a graph or runtime.

**Tech stack:** Rust stable, Edition 2021, standard library (`Arc`, `BTreeMap`,
threads), existing `thiserror`-backed `MuxivaError`, Cargo. No new crate
dependency is required.

**Normative design:**
`docs/design/03-frame-and-ownership-contract.md`. Its validation ranges,
layouts, privacy rules, error codes, and derivation behavior are acceptance
requirements.

## Global execution constraints

- Work only on `codex/stage-3-frames` in a dedicated repository worktree.
- Keep `#![forbid(unsafe_code)]`; do not add an `unsafe` block, allow, or
  exception.
- Do not add Tokio, another async runtime, graph execution, Node/Edge structs,
  queues, scheduling, FFI code, C/C++ headers, serde, live-Frame JSON, media
  SDKs, RTC, FFmpeg, Python, Node-API, or network behavior.
- Do not expose a mutable byte pointer, mutable payload/header field, generic
  string-to-ID conversion, or borrowed data with a lifetime intended to cross
  a future queue/FFI boundary.
- Use `checked_add`, `checked_mul`, and `try_from` before forming payload slice
  ranges. Numeric validation failures return the contract's stable code and do
  not panic.
- Do not change `muxiva-core` logging or add missing Stage 2 error-context
  builders. The only existing foundation correction authorized here is
  removing `Ord`/`PartialOrd` from `Timestamp`, correcting its documentation,
  and routing ordering through `FrameHeader::compare_timestamp`. Preserve all
  other deferred findings in the Stage 3 report.
- Every GREEN step includes formatting, focused Clippy, and focused tests.
- Each listed commit is small and reviewable. Do not combine commits merely to
  reduce the count.

## Planned file set

```text
crates/muxiva-types/src/id.rs
crates/muxiva-types/src/time.rs
crates/muxiva-types/src/schema.rs
crates/muxiva-types/src/frame_buffer.rs
crates/muxiva-types/src/value.rs
crates/muxiva-types/src/extension.rs
crates/muxiva-types/src/lineage.rs
crates/muxiva-types/src/frame/mod.rs
crates/muxiva-types/src/frame/header.rs
crates/muxiva-types/src/frame/audio.rs
crates/muxiva-types/src/frame/video.rs
crates/muxiva-types/src/frame/message.rs
crates/muxiva-types/src/lib.rs
crates/muxiva-types/tests/frame_contract.rs
crates/muxiva-types/tests/frame_derivation.rs
crates/muxiva-types/tests/frame_concurrency.rs
crates/muxiva-examples/src/bin/frames.rs
docs/pre_release_notes/03-frames-and-ownership.md
```

Do not create `frame.rs`; `src/frame/mod.rs` is orchestration and public
dispatch only. Unit tests requiring private Arc observation stay beside
`frame_buffer.rs`; public behavior belongs in the three integration tests.

## Exact public API target

The implementation may reorder private helpers, but the Stage 3 public surface
must match these signatures.

### Foundation additions

```rust
pub struct FrameId(Box<str>);
pub struct EdgeId(Box<str>);
pub struct ClockDomainId(Box<str>);
pub struct ProducerId(Box<str>);

impl FrameId {
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, IdentifierError>;
    pub fn as_str(&self) -> &str;
}
```

`EdgeId`, `ClockDomainId`, and `ProducerId` have the same two inherent methods
plus `Display` and `FromStr`; all remain distinct.

```rust
pub struct SchemaVersion(u32);
impl SchemaVersion {
    pub fn new(value: u32) -> muxiva_types::Result<Self>;
    pub const fn get(self) -> u32;
}

pub struct NamespacedName(Box<str>);
impl NamespacedName {
    pub fn new(value: impl Into<Box<str>>) -> muxiva_types::Result<Self>;
    pub fn as_str(&self) -> &str;
}
```

### Buffer, Value, and metadata

```rust
#[derive(Clone)]
pub struct FrameBuffer(Arc<[u8]>);
impl FrameBuffer {
    pub fn from_vec(bytes: Vec<u8>) -> Self;
    pub fn from_boxed_slice(bytes: Box<[u8]>) -> Self;
    pub fn as_slice(&self) -> &[u8];
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

pub struct FiniteF64(f64);
impl FiniteF64 {
    pub fn new(value: f64) -> muxiva_types::Result<Self>;
    pub const fn get(self) -> f64;
}

pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(FiniteF64),
    String(Box<str>),
    Bytes(FrameBuffer),
    List(Box<[Value]>),
    Map(ValueMap),
}
pub struct ValueMap(BTreeMap<Box<str>, Value>);
impl ValueMap {
    pub fn empty() -> Self;
    pub fn try_from_iter<I, K>(values: I) -> muxiva_types::Result<Self>
    where I: IntoIterator<Item = (K, Value)>, K: Into<Box<str>>;
    pub fn get(&self, key: &str) -> Option<&Value>;
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

pub struct Metadata(BTreeMap<Box<str>, Value>);
impl Metadata {
    pub fn empty() -> Self;
    pub fn try_from_iter<I, K>(values: I) -> muxiva_types::Result<Self>
    where I: IntoIterator<Item = (K, Value)>, K: Into<Box<str>>;
    pub fn get(&self, key: &str) -> Option<&Value>;
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

### Extensions and lineage

```rust
pub enum ExtensionVisibility { Public, Private }
pub enum ExtensionProducer { Core, Node(NodeId), External(ProducerId) }

pub struct Extension;
impl Extension {
    pub fn new(
        key: NamespacedName,
        schema_version: SchemaVersion,
        producer: ExtensionProducer,
        visibility: ExtensionVisibility,
        value: Value,
    ) -> Self;
    pub fn key(&self) -> &NamespacedName;
    pub const fn schema_version(&self) -> SchemaVersion;
    pub fn producer(&self) -> &ExtensionProducer;
    pub const fn visibility(&self) -> ExtensionVisibility;
    pub fn value(&self) -> &Value;
}

pub struct Extensions;
impl Extensions {
    pub fn empty() -> Self;
    pub fn try_from_iter<I>(extensions: I) -> muxiva_types::Result<Self>
    where I: IntoIterator<Item = Extension>;
    pub fn get(&self, key: &NamespacedName, version: SchemaVersion) -> Option<&Extension>;
    pub fn iter(&self) -> impl Iterator<Item = &Extension>;
    pub fn public_iter(&self) -> impl Iterator<Item = &Extension>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}

pub struct TransformOrigin;
impl TransformOrigin {
    pub fn new(
        node_id: Option<NodeId>,
        edge_id: Option<EdgeId>,
    ) -> muxiva_types::Result<Self>;
    pub fn node_id(&self) -> Option<&NodeId>;
    pub fn edge_id(&self) -> Option<&EdgeId>;
}

pub struct LineageEntry;
impl LineageEntry {
    pub fn new(
        parent_frame_id: FrameId,
        origin: TransformOrigin,
        reason: impl Into<Box<str>>,
    ) -> muxiva_types::Result<Self>;
    pub fn parent_frame_id(&self) -> &FrameId;
    pub fn origin(&self) -> &TransformOrigin;
    pub fn reason(&self) -> &str;
}

pub struct Lineage;
impl Lineage {
    pub fn empty() -> Self;
    pub fn iter(&self) -> impl Iterator<Item = &LineageEntry>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

`Lineage::from_entries(entries: Vec<LineageEntry>) -> Self` and
`Lineage::append(self, entry: LineageEntry) -> Self` are `pub(crate)` so
ordinary callers use Frame derivation rather than manufacturing history after
construction.

### Header and payloads

```rust
// Derives Clone, Copy, Debug, Eq, Hash, and PartialEq; deliberately not
// Ord or PartialOrd.
pub struct Timestamp(i64);
impl Timestamp {
    pub const fn from_nanos(nanos: i64) -> Self;
    pub const fn as_nanos(self) -> i64;
    pub const fn checked_add(self, nanos: i64) -> Option<Self>;
}

pub enum ClockKind { Monotonic, MediaRelative, WallClock }
pub struct ClockDomain;
impl ClockDomain {
    pub fn new(id: ClockDomainId, kind: ClockKind) -> Self;
    pub fn id(&self) -> &ClockDomainId;
    pub const fn kind(&self) -> ClockKind;
}

pub enum FrameType { Audio, Video, Text, Byte, Signal, Event }

pub struct FrameHeader;
impl FrameHeader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        frame_id: FrameId,
        timestamp: Timestamp,
        clock_domain: ClockDomain,
        sequence_id: SequenceId,
        stream_id: StreamId,
        trace_id: TraceId,
        frame_type: FrameType,
        metadata: Metadata,
        extensions: Extensions,
        lineage: Lineage,
    ) -> muxiva_types::Result<Self>;
    pub fn frame_id(&self) -> &FrameId;
    pub const fn timestamp(&self) -> Timestamp;
    pub fn clock_domain(&self) -> &ClockDomain;
    pub const fn sequence_id(&self) -> SequenceId;
    pub fn stream_id(&self) -> &StreamId;
    pub fn trace_id(&self) -> &TraceId;
    pub const fn frame_type(&self) -> FrameType;
    pub fn metadata(&self) -> &Metadata;
    pub fn extensions(&self) -> &Extensions;
    pub fn lineage(&self) -> &Lineage;
    pub fn compare_timestamp(
        &self,
        other: &FrameHeader,
    ) -> muxiva_types::Result<std::cmp::Ordering>;
}

pub enum PcmSampleFormat { U8, I16Le, I24Le, I32Le, F32Le, F64Le }
impl PcmSampleFormat { pub const fn bytes_per_sample(self) -> usize; }
pub enum AudioLayout { Interleaved, Planar }

pub struct AudioData;
impl AudioData {
    pub fn new(
        buffer: FrameBuffer,
        sample_rate_hz: u32,
        channels: u16,
        sample_format: PcmSampleFormat,
        layout: AudioLayout,
        samples_per_channel: u64,
    ) -> muxiva_types::Result<Self>;
    pub fn buffer(&self) -> &FrameBuffer;
    pub const fn sample_rate_hz(&self) -> u32;
    pub const fn channels(&self) -> u16;
    pub const fn sample_format(&self) -> PcmSampleFormat;
    pub const fn layout(&self) -> AudioLayout;
    pub const fn samples_per_channel(&self) -> u64;
    pub const fn duration_ns(&self) -> u64;
    pub fn plane_bytes(&self, plane: u16) -> muxiva_types::Result<&[u8]>;
}

pub enum PixelFormat { Rgba8, Yuv420p }
pub struct VideoPlane;
impl VideoPlane {
    pub const fn offset(&self) -> usize;
    pub const fn stride(&self) -> usize;
    pub const fn row_bytes(&self) -> usize;
    pub const fn rows(&self) -> u32;
}

pub enum VideoLayout {
    Rgba8 { plane: VideoPlane },
    Yuv420p { y: VideoPlane, u: VideoPlane, v: VideoPlane },
}
pub struct VideoData;
impl VideoData {
    pub fn rgba8(
        buffer: FrameBuffer, width: u32, height: u32, stride: usize,
    ) -> muxiva_types::Result<Self>;
    pub fn yuv420p(
        buffer: FrameBuffer,
        width: u32,
        height: u32,
        y_stride: usize,
        u_stride: usize,
        v_stride: usize,
    ) -> muxiva_types::Result<Self>;
    pub fn buffer(&self) -> &FrameBuffer;
    pub const fn width(&self) -> u32;
    pub const fn height(&self) -> u32;
    pub const fn pixel_format(&self) -> PixelFormat;
    pub fn layout(&self) -> &VideoLayout;
    pub fn plane_bytes(&self, plane: &VideoPlane) -> muxiva_types::Result<&[u8]>;
}

pub struct TextData;
impl TextData {
    pub fn new(text: impl Into<Box<str>>) -> Self;
    pub fn from_utf8(bytes: FrameBuffer) -> muxiva_types::Result<Self>;
    pub fn as_str(&self) -> &str;
}

pub struct MediaType;
impl MediaType {
    pub fn new(value: impl Into<Box<str>>) -> muxiva_types::Result<Self>;
    pub fn as_str(&self) -> &str;
}

pub struct ByteData;
impl ByteData {
    pub fn new(buffer: FrameBuffer, media_type: Option<MediaType>) -> Self;
    pub fn buffer(&self) -> &FrameBuffer;
    pub fn media_type(&self) -> Option<&MediaType>;
}

pub struct SignalData;
impl SignalData {
    pub fn new(
        name: NamespacedName,
        schema_version: SchemaVersion,
        source: NodeId,
        payload: Value,
    ) -> Self;
    pub fn name(&self) -> &NamespacedName;
    pub const fn schema_version(&self) -> SchemaVersion;
    pub fn source(&self) -> &NodeId;
    pub fn payload(&self) -> &Value;
}

pub struct EventData;
impl EventData {
    pub fn new(
        topic: NamespacedName,
        schema_version: SchemaVersion,
        source: NodeId,
        payload: Value,
    ) -> Self;
    pub fn topic(&self) -> &NamespacedName;
    pub const fn schema_version(&self) -> SchemaVersion;
    pub fn source(&self) -> &NodeId;
    pub fn payload(&self) -> &Value;
}
```

`Timestamp` no longer implements `Ord` or `PartialOrd`. This is an intentional
pre-1.0 correction to the Stage 2 public surface. Header comparison returns
`MUXIVA-FRM-CLOCK-DOMAIN` unless both complete `ClockDomain` values are equal;
equal domains compare the signed nanoseconds. Do not add a bare Timestamp
comparison helper or implement clock conversion.

### Frame assembly, derivation, and views

`AudioFrame`, `VideoFrame`, `TextFrame`, `ByteFrame`, `SignalFrame`, and
`EventFrame` each have `header()` plus `data()` returning the corresponding
payload type. They are created by `Frame::new`, not public unchecked
constructors.

```rust
pub enum FramePayload {
    Audio(AudioData), Video(VideoData), Text(TextData), Byte(ByteData),
    Signal(SignalData), Event(EventData),
}
impl FramePayload { pub const fn frame_type(&self) -> FrameType; }

pub enum Frame {
    Audio(AudioFrame), Video(VideoFrame), Text(TextFrame), Byte(ByteFrame),
    Signal(SignalFrame), Event(EventFrame),
}

impl Frame {
    pub fn new(header: FrameHeader, payload: FramePayload) -> muxiva_types::Result<Self>;
    pub const fn frame_type(&self) -> FrameType;
    pub fn header(&self) -> &FrameHeader;
    pub fn as_audio(&self) -> Option<&AudioFrame>;
    pub fn as_video(&self) -> Option<&VideoFrame>;
    pub fn as_text(&self) -> Option<&TextFrame>;
    pub fn as_byte(&self) -> Option<&ByteFrame>;
    pub fn as_signal(&self) -> Option<&SignalFrame>;
    pub fn as_event(&self) -> Option<&EventFrame>;
    pub fn ensure_type(&self, expected: FrameType) -> muxiva_types::Result<()>;
    pub fn derive(&self, derivation: FrameDerivation) -> muxiva_types::Result<Self>;
    pub fn public_view(&self) -> PublicFrameView<'_>;
    pub fn log_safe_view(&self) -> LogSafeFrameView<'_>;
}

pub struct FrameDerivation;
impl FrameDerivation {
    pub fn new(
        new_frame_id: FrameId,
        timestamp: Timestamp,
        sequence_id: SequenceId,
        origin: TransformOrigin,
        reason: impl Into<Box<str>>,
    ) -> muxiva_types::Result<Self>;
    pub fn with_metadata(self, metadata: Metadata) -> Self;
    pub fn with_extensions(self, extensions: Extensions) -> Self;
    pub fn with_payload(self, payload: FramePayload) -> Self;
}

pub struct PublicFrameView<'a>;
impl<'a> PublicFrameView<'a> {
    pub fn header(&self) -> PublicFrameHeaderView<'a>;
    pub const fn frame_type(&self) -> FrameType;
}

pub struct PublicFrameHeaderView<'a>;
impl<'a> PublicFrameHeaderView<'a> {
    pub fn frame_id(&self) -> &'a FrameId;
    pub const fn timestamp(&self) -> Timestamp;
    pub fn clock_domain(&self) -> &'a ClockDomain;
    pub const fn sequence_id(&self) -> SequenceId;
    pub fn stream_id(&self) -> &'a StreamId;
    pub fn trace_id(&self) -> &'a TraceId;
    pub fn metadata(&self) -> &'a Metadata;
    pub fn extensions(&self) -> impl Iterator<Item = &'a Extension>;
    pub fn lineage(&self) -> &'a Lineage;
}

pub struct LogSafeFrameView<'a>;
impl<'a> LogSafeFrameView<'a> {
    pub fn frame_id(&self) -> &'a FrameId;
    pub fn stream_id(&self) -> &'a StreamId;
    pub fn trace_id(&self) -> &'a TraceId;
    pub const fn frame_type(&self) -> FrameType;
    pub const fn timestamp(&self) -> Timestamp;
    pub fn clock_domain(&self) -> &'a ClockDomain;
    pub const fn sequence_id(&self) -> SequenceId;
    pub fn payload_byte_len(&self) -> Option<usize>;
    pub fn metadata_key_count(&self) -> usize;
    pub fn public_extension_count(&self) -> usize;
    pub fn lineage_count(&self) -> usize;
}
```

`PublicFrameView` excludes private extensions but includes explicitly public
extension values. `LogSafeFrameView` excludes all payload/value contents,
metadata values, all extension values, private keys, and lineage reasons.
Custom `Debug` for `Frame`, `FrameHeader`, `Extension`, and `FrameBuffer`
follows this redaction policy and never delegates to a derived Debug that
prints private data.

---

## Task 1: Strong frame IDs, schema versions, and namespaces

**Files:**

- Modify: `crates/muxiva-types/src/id.rs`
- Create: `crates/muxiva-types/src/schema.rs`
- Modify: `crates/muxiva-types/src/lib.rs`

- [ ] **Step 1 — RED: add ID separation and namespace tests**

Add unit tests that construct every new ID, reject the existing invalid-ID
classes, reject schema version zero, and exercise namespace segment edges.
Add this compile-fail doc test to `FrameId`:

```rust
/// ```compile_fail
/// use muxiva_types::{EdgeId, FrameId};
/// fn needs_edge(_: EdgeId) {}
/// let frame = FrameId::new("frame-1").unwrap();
/// needs_edge(frame);
/// ```
```

Run:

```bash
cargo test -p muxiva-types id::tests -- --nocapture
cargo test -p muxiva-types schema::tests -- --nocapture
```

Expected RED: unresolved new ID/schema symbols or absent module.

- [ ] **Step 2 — GREEN: implement and export exact types**

Reuse the private `identifier_type!` macro for all four IDs. Implement the
contract grammar in one private `validate_namespace` helper. Return:

```rust
MuxivaError::new(
    ErrorCategory::Validation,
    "MUXIVA-FRM-NAMESPACE",
    "name must be a qualified ASCII namespace",
)
```

for every namespace grammar failure and `MUXIVA-FRM-SCHEMA-VERSION` for zero.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types id::tests -- --nocapture
cargo test -p muxiva-types schema::tests -- --nocapture
cargo test -p muxiva-types --doc
```

Expected GREEN: all pass, including nominal type separation.

- [ ] **Step 3 — commit**

```bash
git add crates/muxiva-types/src/id.rs crates/muxiva-types/src/schema.rs crates/muxiva-types/src/lib.rs
git commit -m "feat(types): add frame identity and schema values"
```

## Task 2: Arc-backed immutable buffers, Value, and metadata

**Files:**

- Create: `crates/muxiva-types/src/frame_buffer.rs`
- Create: `crates/muxiva-types/src/value.rs`
- Modify: `crates/muxiva-types/src/lib.rs`

- [ ] **Step 1 — RED: write buffer lifetime tests beside the module**

The unit test may inspect the private Arc, while public API stays minimal:

```rust
#[test]
fn allocation_releases_after_last_clone() {
    let buffer = FrameBuffer::from_vec(vec![1, 2, 3]);
    let weak = Arc::downgrade(&buffer.0);
    let clone = buffer.clone();
    drop(buffer);
    assert!(weak.upgrade().is_some());
    drop(clone);
    assert!(weak.upgrade().is_none());
}
```

Also test clone pointer equality through `as_slice().as_ptr()`, empty buffer,
and length-only Debug.

Run:

```bash
cargo test -p muxiva-types frame_buffer::tests -- --nocapture
```

Expected RED: `FrameBuffer` does not exist.

- [ ] **Step 2 — GREEN: implement FrameBuffer with no mutable escape**

Move `Vec<u8>` or `Box<[u8]>` into `Arc<[u8]>`. Implement `PartialEq` by byte
content and custom Debug as `FrameBuffer { len: N }`; do not expose the Arc or
reference count.

- [ ] **Step 3 — RED: add Value/Metadata validation tests**

```rust
#[test]
fn values_reject_non_finite_numbers_and_bad_keys() {
    assert_eq!(
        FiniteF64::new(f64::NAN).unwrap_err().code(),
        "MUXIVA-FRM-VALUE-NUMBER"
    );
    let error = ValueMap::try_from_iter([(Box::<str>::from(""), Value::Null)])
        .unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-VALUE-KEY");
}
```

Test every Value variant and deterministic map/metadata iteration.

Run:

```bash
cargo test -p muxiva-types value::tests -- --nocapture
```

Expected RED: Value and Metadata are absent.

- [ ] **Step 4 — GREEN: implement closed owned values**

Use one private key validator for both map and metadata keys. Ensure the enum
cannot receive a raw `f64`; only `FiniteF64` can inhabit `Value::Float`.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types frame_buffer::tests -- --nocapture
cargo test -p muxiva-types value::tests -- --nocapture
```

Expected GREEN: buffer and value tests pass without new dependencies.

- [ ] **Step 5 — commit**

```bash
git add crates/muxiva-types/src/frame_buffer.rs crates/muxiva-types/src/value.rs crates/muxiva-types/src/lib.rs
git commit -m "feat(types): add immutable frame buffers and values"
```

## Task 3: Extensions and lineage

**Files:**

- Create: `crates/muxiva-types/src/extension.rs`
- Create: `crates/muxiva-types/src/lineage.rs`
- Modify: `crates/muxiva-types/src/lib.rs`

- [ ] **Step 1 — RED: add extension uniqueness and visibility tests**

Construct one public and one private extension. Assert `iter().count() == 2`,
`public_iter().count() == 1`, and duplicate key/version returns
`MUXIVA-FRM-EXTENSION-DUPLICATE`. Assert two versions of one key are accepted.

Run:

```bash
cargo test -p muxiva-types extension::tests -- --nocapture
```

Expected RED: extension module absent.

- [ ] **Step 2 — GREEN: implement immutable extension collection**

Preserve caller order in `Box<[Extension]>`; use a private `BTreeSet` only
during construction to detect duplicate `(NamespacedName, SchemaVersion)`.
Custom Debug for a private extension prints key as `"<private>"` and never
prints its Value; public Debug prints key/schema/producer but still omits Value.

- [ ] **Step 3 — RED: add origin/reason/lineage tests**

```rust
#[test]
fn transform_origin_requires_attribution() {
    let error = TransformOrigin::new(None, None).unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-LINEAGE-ORIGIN");
}
```

Test Node-only, Edge-only, both, empty/257-byte/control-character reasons, and
ordered entries through the crate-private constructor.

Run:

```bash
cargo test -p muxiva-types lineage::tests -- --nocapture
```

Expected RED: lineage types absent.

- [ ] **Step 4 — GREEN: implement validated lineage values**

Keep construction and append immutable by consuming a `Lineage` internally.
No method accepts raw payload or Value as a reason.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types extension::tests -- --nocapture
cargo test -p muxiva-types lineage::tests -- --nocapture
```

Expected GREEN: collection, visibility, and lineage tests pass.

- [ ] **Step 5 — commit**

```bash
git add crates/muxiva-types/src/extension.rs crates/muxiva-types/src/lineage.rs crates/muxiva-types/src/lib.rs
git commit -m "feat(types): add frame extensions and lineage"
```

## Task 4: Header, audio validation, and exact arithmetic

**Files:**

- Modify: `crates/muxiva-types/src/time.rs`
- Create: `crates/muxiva-types/src/frame/mod.rs`
- Create: `crates/muxiva-types/src/frame/header.rs`
- Create: `crates/muxiva-types/src/frame/audio.rs`
- Modify: `crates/muxiva-types/src/lib.rs`
- Create: `crates/muxiva-types/tests/frame_contract.rs`

- [ ] **Step 1 — RED: prohibit raw timestamp ordering and test checked comparison**

Change the `Timestamp` documentation example in `time.rs` to include this
compile-fail test. With the Stage 2 derives still present, the doc test is RED
because the body incorrectly compiles:

```rust
/// ```compile_fail
/// use muxiva_types::Timestamp;
/// let earlier = Timestamp::from_nanos(1);
/// let later = Timestamp::from_nanos(2);
/// let _ = earlier < later;
/// ```
```

In `tests/frame_contract.rs`, add a helper that builds an empty header from
explicit values and these behavioral tests:

```rust
use std::cmp::Ordering;

#[test]
fn header_compares_timestamps_only_inside_one_clock_domain() {
    let domain = ClockDomain::new(
        ClockDomainId::new("capture.audio").unwrap(),
        ClockKind::MediaRelative,
    );
    let earlier = header_in(domain.clone(), Timestamp::from_nanos(-1));
    let later = header_in(domain, Timestamp::from_nanos(2));
    assert_eq!(earlier.compare_timestamp(&later).unwrap(), Ordering::Less);
    assert_eq!(later.compare_timestamp(&earlier).unwrap(), Ordering::Greater);
    assert_eq!(earlier.compare_timestamp(&earlier).unwrap(), Ordering::Equal);
}

#[test]
fn header_rejects_same_kind_with_different_clock_ids() {
    let left = header_in(media_domain("capture.left"), Timestamp::from_nanos(1));
    let right = header_in(media_domain("capture.right"), Timestamp::from_nanos(2));
    let error = left.compare_timestamp(&right).unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-CLOCK-DOMAIN");
}
```

Put the self-parent cycle test in `frame/header.rs`'s unit-test module, not the
public integration test. The unit test constructs one entry with
`Lineage::from_entries`, sets `parent_frame_id` equal to the new header ID, and
asserts `MUXIVA-FRM-LINEAGE-CYCLE`. This is the only test allowed to use that
crate-private constructor. Keep the later public integration assertion that
derivation rejects reuse of the direct parent ID.

```rust
#[test]
fn header_rejects_self_parent_lineage() {
    let frame_id = FrameId::new("frame-cycle").unwrap();
    let origin = TransformOrigin::new(
        Some(NodeId::new("normalize").unwrap()),
        None,
    ).unwrap();
    let entry = LineageEntry::new(frame_id.clone(), origin, "normalize").unwrap();
    let lineage = Lineage::from_entries(vec![entry]);
    let error = header_with_lineage(frame_id, lineage).unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-LINEAGE-CYCLE");
}
```

Run:

```bash
cargo test -p muxiva-types --doc
cargo test -p muxiva-types frame::header::tests -- --nocapture
cargo test -p muxiva-types --test frame_contract header -- --nocapture
```

Expected RED: the raw comparison doc test compiles unexpectedly and the new
header comparison surface is absent.

- [ ] **Step 2 — GREEN: remove raw ordering and implement domain checking**

In `time.rs`, remove only `Ord` and `PartialOrd` from `Timestamp`'s derives and
correct its doc comment to say the signed nanosecond value requires an
explicit clock domain. Keep `SequenceId` ordering unchanged.

Implement `ClockKind`, `ClockDomain`, `FrameType`, and `FrameHeader`. Derive
Clone/equality as appropriate; hand-write Debug to print counts and scalar
identity only. `compare_timestamp` checks complete domain equality before
calling `self.timestamp.as_nanos().cmp(&other.timestamp.as_nanos())`. A
different domain returns:

```rust
MuxivaError::new(
    ErrorCategory::Validation,
    "MUXIVA-FRM-CLOCK-DOMAIN",
    "timestamps from different clock domains cannot be ordered",
)
```

Attach only the two domain IDs as error context; do not perform conversion or
silently compare equal clock kinds.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --doc
cargo test -p muxiva-types frame::header::tests -- --nocapture
cargo test -p muxiva-types --test frame_contract header -- --nocapture
```

Expected GREEN: raw `<` is rejected by the compiler, same-domain signed values
order correctly, different IDs reject, and the crate-private self-parent test
passes.

- [ ] **Step 3 — RED: add audio table tests**

The valid case fixes the length calculation:

```rust
#[test]
fn constructs_interleaved_pcm_and_duration() {
    let audio = AudioData::new(
        FrameBuffer::from_vec(vec![0; 1_920]),
        48_000,
        2,
        PcmSampleFormat::I16Le,
        AudioLayout::Interleaved,
        480,
    ).unwrap();
    assert_eq!(audio.duration_ns(), 10_000_000);
}
```

Table-test zero and 768,001 rates, zero and 1,025 channels, zero samples,
short/trailing payloads, and `u64::MAX` samples causing
`MUXIVA-FRM-ARITHMETIC` before allocation or slicing. Test plane 0 as the only
valid interleaved plane, every `0..channels` planar plane, and invalid indices
for both layouts; invalid indices must return `MUXIVA-FRM-AUDIO-PLANE`, never
`MUXIVA-FRM-AUDIO-CHANNELS`.

Run:

```bash
cargo test -p muxiva-types --test frame_contract audio -- --nocapture
```

Expected RED: AudioData and its enums unresolved.

- [ ] **Step 4 — GREEN: implement checked audio layout**

Use helpers shaped as:

```rust
fn checked_product(left: u64, right: u64) -> Result<u64> {
    left.checked_mul(right).ok_or_else(|| {
        MuxivaError::new(
            ErrorCategory::Validation,
            "MUXIVA-FRM-ARITHMETIC",
            "frame size arithmetic overflowed",
        )
    })
}
```

Convert to `usize` with `usize::try_from` and the same error code. Length
mismatch errors attach decimal `expected_bytes` and `actual_bytes`, never
payload content. `plane_bytes` returns the whole buffer for interleaved layout
when `plane == 0`; any other interleaved plane returns
`MUXIVA-FRM-AUDIO-PLANE`. For planar layout it returns the checked contiguous
channel plane and accepts `0..channels`. Document this behavior on the method.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --test frame_contract header -- --nocapture
cargo test -p muxiva-types --test frame_contract audio -- --nocapture
```

Expected GREEN: all header/audio cases pass and no panic is observed.

- [ ] **Step 5 — commit**

```bash
git add crates/muxiva-types/src/time.rs crates/muxiva-types/src/frame crates/muxiva-types/src/lib.rs crates/muxiva-types/tests/frame_contract.rs
git commit -m "feat(types): validate frame headers and PCM audio"
```

## Task 5: RGBA8 and YUV420P video layouts

**Files:**

- Create: `crates/muxiva-types/src/frame/video.rs`
- Modify: `crates/muxiva-types/src/frame/mod.rs`
- Modify: `crates/muxiva-types/src/lib.rs`
- Modify: `crates/muxiva-types/tests/frame_contract.rs`

- [ ] **Step 1 — RED: add exact plane/stride/overflow tests**

Valid RGBA8 case: width 2, height 2, stride 8, buffer length 16. Valid YUV420P
case: width 4, height 2, strides 4/2/2, buffer length 12, with offsets
Y=0/U=8/V=10. Assert every plane's row bytes and rows.

Add tables for zero width/height, odd YUV dimensions, RGBA stride below
`width * 4`, each Y/U/V short stride, short/trailing payload, and
`u32::MAX` dimensions with huge strides. Overflow must return
`MUXIVA-FRM-ARITHMETIC`; impossible allocation is never attempted.

Construct two separate valid `VideoData` values, obtain a `VideoPlane`
reference from the first value's `layout`, and pass it to the second value's
`plane_bytes`. Assert `MUXIVA-FRM-VIDEO-PLANE`, even when both layouts have the
same dimensions and scalar plane fields. Valid own-layout plane references
must return their full stride-including ranges.

Run:

```bash
cargo test -p muxiva-types --test frame_contract video -- --nocapture
```

Expected RED: video module/types absent.

- [ ] **Step 2 — GREEN: calculate planes before slicing**

Use shared private checked-size helpers from `frame/mod.rs`. Construct private
`VideoPlane` values only after dimension and stride checks. Calculate offsets
and total length with checked arithmetic. `plane_bytes` verifies the plane is
one of the current layout's borrowed descriptor instances with
`std::ptr::eq` before returning its full stride-including range; scalar
descriptor equality is insufficient. A foreign reference returns
`MUXIVA-FRM-VIDEO-PLANE`. Reserve `MUXIVA-FRM-VIDEO-LENGTH` solely for constructor
payload-length mismatch. This uses safe pointer identity and exposes no raw
pointer.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --test frame_contract video -- --nocapture
```

Expected GREEN: layouts and all invalid cases pass.

- [ ] **Step 3 — commit**

```bash
git add crates/muxiva-types/src/frame/video.rs crates/muxiva-types/src/frame/mod.rs crates/muxiva-types/src/lib.rs crates/muxiva-types/tests/frame_contract.rs
git commit -m "feat(types): validate immutable video layouts"
```

## Task 6: Message payloads and six-variant Frame assembly

**Files:**

- Create: `crates/muxiva-types/src/frame/message.rs`
- Modify: `crates/muxiva-types/src/frame/mod.rs`
- Modify: `crates/muxiva-types/src/lib.rs`
- Modify: `crates/muxiva-types/tests/frame_contract.rs`

- [ ] **Step 1 — RED: add text, bytes, signal, and event tests**

Test invalid UTF-8 `[0xff]`, valid/invalid media types including `audio/pcm`,
an empty Byte payload, namespaced Signal/Event values, and all Value payload
variants. Assert Signal/Event timestamps come only from the common header.

Run:

```bash
cargo test -p muxiva-types --test frame_contract messages -- --nocapture
```

Expected RED: message payload types absent.

- [ ] **Step 2 — GREEN: implement owned message payloads**

For UTF-8 conversion, validate with `std::str::from_utf8`, then allocate the
owned `Box<str>`; do not keep an unsafe or borrowed view. Keep Byte payloads in
FrameBuffer. Signal/Event constructors are infallible because their component
types were already validated.

- [ ] **Step 3 — RED: construct every Frame variant and type mismatch**

Create a table containing all six `FramePayload` variants and matching
headers. Assert `frame_type`, `header`, and exactly one typed `as_*` accessor.
Then pair an Audio header with Text payload:

```rust
let error = Frame::new(audio_header, FramePayload::Text(TextData::new("hello")))
    .unwrap_err();
assert_eq!(error.code(), "MUXIVA-FRM-TYPE-MISMATCH");
```

Also call `ensure_type` with matching and differing types.

Run:

```bash
cargo test -p muxiva-types --test frame_contract frame_variants -- --nocapture
```

Expected RED: Frame assembly incomplete.

- [ ] **Step 4 — GREEN: implement checked Frame dispatch**

Use one private match to construct concrete wrappers; do not expose wrapper
fields or unchecked constructors. Implement Frame Debug using the log-safe
summary policy even before derivation/view methods are complete.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --test frame_contract -- --nocapture
```

Expected GREEN: header, media, messages, six variants, and type gate pass.

- [ ] **Step 5 — commit**

```bash
git add crates/muxiva-types/src/frame/message.rs crates/muxiva-types/src/frame/mod.rs crates/muxiva-types/src/lib.rs crates/muxiva-types/tests/frame_contract.rs
git commit -m "feat(types): assemble six immutable frame variants"
```

## Task 7: Derivation, unknown-extension preservation, and safe views

**Files:**

- Modify: `crates/muxiva-types/src/frame/mod.rs`
- Modify: `crates/muxiva-types/src/frame/header.rs`
- Create: `crates/muxiva-types/tests/frame_derivation.rs`

- [ ] **Step 1 — RED: add derivation invariants**

Build a parent Byte Frame with metadata, one unrecognized public extension,
and one private extension. Derive it with a new ID, Node+Edge origin, and a new
payload buffer. Assert:

- parent ID, payload, extension list, and empty lineage are unchanged;
- child stream/trace/clock/metadata are equal to the parent by default;
- the two extensions compare equal and retain input order;
- child lineage length is one, parent ID matches, origin matches, and reason
  is exact;
- child buffer differs when payload is replaced; and
- reusing the parent ID returns `MUXIVA-FRM-DERIVATION-ID`.

Add a second derivation that overrides metadata/extensions and transforms Byte
to Text, proving the child header type comes from the new payload.

Run:

```bash
cargo test -p muxiva-types --test frame_derivation derivation -- --nocapture
```

Expected RED: FrameDerivation/derive absent.

- [ ] **Step 2 — GREEN: implement consume-and-return derivation**

Keep optional overrides private in `FrameDerivation`. Clone the parent's
validated payload when no override is present; FrameBuffer clone is Arc
sharing. Append through the crate-private Lineage method. Call `Frame::new` at
the end so there is only one header/payload compatibility gate.

- [ ] **Step 3 — RED: prove privacy boundaries**

```rust
#[test]
fn private_extension_is_absent_from_default_views() {
    let frame = frame_with_public_and_private_extensions();
    let public_keys: Vec<_> = frame.public_view().header()
        .extensions().map(|extension| extension.key().as_str()).collect();
    assert_eq!(public_keys, vec!["com.example.public"]);

    let rendered = format!("{:?}", frame);
    assert!(!rendered.contains("com.example.private_context"));
    assert!(!rendered.contains("private-secret"));
    assert!(!rendered.contains("public-value"));

    let header_rendered = format!("{:?}", frame.header());
    assert!(!header_rendered.contains("com.example.private_context"));
    assert!(!header_rendered.contains("private-secret"));
    assert!(!header_rendered.contains("public-value"));

    let private = frame.header().extensions().iter()
        .find(|extension| extension.visibility() == ExtensionVisibility::Private)
        .unwrap();
    let extension_rendered = format!("{:?}", private);
    assert!(!extension_rendered.contains("com.example.private_context"));
    assert!(!extension_rendered.contains("private-secret"));
}
```

Also assert `header().extensions().iter()` still lets the receiving caller
read the private extension, `log_safe_view` reports only the public count, and
the public Extension's direct Debug includes its public key/schema/producer
but omits its Value. These direct assertions prevent Frame-level redaction
from hiding a leaking `FrameHeader` or `Extension` implementation.

Run:

```bash
cargo test -p muxiva-types --test frame_derivation privacy -- --nocapture
```

Expected RED: view types absent or Debug leaks values.

- [ ] **Step 4 — GREEN: implement borrowed filtered views**

Views store `&Frame` or `&FrameHeader`; they do not clone collections. Their
extension iterator is the existing `public_iter` filter. `payload_byte_len`
returns buffer length for Audio/Video/Byte, UTF-8 byte length for Text, and
`None` for Signal/Event.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --test frame_derivation -- --nocapture
```

Expected GREEN: derivation, preservation, and privacy tests pass.

- [ ] **Step 5 — commit**

```bash
git add crates/muxiva-types/src/frame crates/muxiva-types/tests/frame_derivation.rs
git commit -m "feat(types): derive frames with safe diagnostic views"
```

## Task 8: Move/clone and concurrent-read acceptance tests

**Files:**

- Create: `crates/muxiva-types/tests/frame_concurrency.rs`

- [ ] **Step 1 — RED: add public ownership tests**

The test must move a Frame through a function, clone it, and use
`Arc<Barrier>` plus eight scoped threads to repeatedly read header fields and
payload bytes. It must not wrap the Frame in a Mutex/RwLock.

Core test shape:

```rust
std::thread::scope(|scope| {
    for _ in 0..8 {
        let shared = frame.clone();
        scope.spawn(move || {
            for _ in 0..1_000 {
                assert_eq!(shared.as_byte().unwrap().data().buffer().as_slice(), &[1, 2, 3]);
            }
        });
    }
});
```

If `Frame` is not yet Clone, the test is RED for the intended reason. Also add
a compile-fail doc test demonstrating no mutable slice API exists by trying to
call `as_slice()[0] = 9`.

Run:

```bash
cargo test -p muxiva-types --test frame_concurrency -- --nocapture
cargo test -p muxiva-types --doc
```

Expected RED: Clone/Send/Sync or compile-fail documentation is incomplete.

- [ ] **Step 2 — GREEN: derive safe clones only**

Derive/implement Clone for immutable owned components. Do not add locks or
interior mutability. Add a test-only compile assertion:

```rust
fn assert_send_sync<T: Send + Sync>() {}
assert_send_sync::<Frame>();
assert_send_sync::<FrameBuffer>();
```

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-types --all-targets -- -D warnings
cargo test -p muxiva-types --test frame_concurrency -- --nocapture
cargo test -p muxiva-types --doc
```

Expected GREEN: concurrent reads and immutability compile checks pass under
the default test runner.

- [ ] **Step 3 — commit**

```bash
git add crates/muxiva-types/tests/frame_concurrency.rs crates/muxiva-types/src
git commit -m "test(types): verify frame ownership and concurrent reads"
```

## Task 9: Minimal construction/derivation example

**Files:**

- Create: `crates/muxiva-examples/src/bin/frames.rs`

- [ ] **Step 1 — RED: compile the absent example**

Run:

```bash
cargo run -p muxiva-examples --bin frames
```

Expected RED: Cargo reports no `frames` binary.

- [ ] **Step 2 — GREEN: add the construction-only example**

Use explicit values: 48,000 Hz, mono `I16Le`, interleaved, 480 samples, 960
zero bytes, media-relative `ClockDomainId("capture.audio")`, public unknown
extension `com.example.future`, private extension
`com.example.private_context`, source Frame ID `frame-1`, derived ID `frame-2`,
Node `normalize`, Edge `capture-to-normalize`, and reason `normalize-volume`.
Replace the child audio buffer with a separately allocated 960-byte buffer.

End with exactly one non-sensitive output line whose stable prefix is:

```text
Muxiva derived frame: frame-2 Audio lineage=1
```

Use assertions to prove the parent remains `frame-1` with empty lineage and
the child retains `com.example.future`. Do not initialize tracing, serialize a
Frame, create a graph, or spawn a thread.

Run:

```bash
cargo fmt --all --check
cargo clippy -p muxiva-examples --all-targets -- -D warnings
cargo test -p muxiva-examples --all-targets
cargo run -p muxiva-examples --bin frames
```

Expected GREEN: all pass and the one output line has the prefix above.

- [ ] **Step 3 — commit**

```bash
git add crates/muxiva-examples/src/bin/frames.rs
git commit -m "feat(examples): construct and derive immutable frames"
```

## Task 10: Stage report, audit, and stop gate

**Files:**

- Create: `docs/pre_release_notes/03-frames-and-ownership.md`

- [ ] **Step 1 — write the report from fresh evidence**

Record exact delivered files, public API, validation/error table, ownership
and thread model, Copy and Retain/Release design-only status, public/private
view behavior, test counts, dependency tree, example output, commits, and
risks. State explicitly that live Frames have no serde/JSON implementation and
that Stage 4 has not started.

Record removal of `Timestamp: Ord + PartialOrd` as a pre-1.0 breaking
correction, list raw comparison/sorting/min/max as affected surfaces, and give
`FrameHeader::compare_timestamp` as the migration. Include fresh evidence for
the compile-fail raw-ordering test plus same-domain and different-domain
behavior. Mark the Stage 2 timestamp wording/ordering issue resolved by this
stage; carry every other Stage 2 deferred finding verbatim in substance.
Separate newly observed Stage 3 concerns from inherited debt. Do not claim CI
execution unless an actual remote CI result is available.

- [ ] **Step 2 — run focused acceptance**

```bash
cargo test -p muxiva-types --all-targets
cargo test -p muxiva-types --doc
cargo test -p muxiva-examples --all-targets
cargo run -p muxiva-examples --bin frames
```

Expected GREEN: all tests pass; example output contains only the log-safe
summary line.

- [ ] **Step 3 — run complete quality gates**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo tree --workspace
```

Expected GREEN: no warning or test failure; dependency tree has no Tokio,
serde, async runtime, FFI, media, RTC, or FFmpeg package.

- [ ] **Step 4 — run forbidden-capability and immutability audits**

```bash
if rg -n 'unsafe\s*\{|allow\s*\(unsafe_code\)|tokio|serde::|derive[^\n]*(Serialize|Deserialize)|extern\s+"C"|no_mangle|GraphRunner|GraphBuilder|struct\s+Node|struct\s+Edge|VecDeque|mpsc|ffmpeg|webrtc|pyo3|napi' crates; then exit 1; fi
if rg -n 'pub fn [^\n]*(&mut|as_mut|mut_ptr)|->\s*(&mut|\*mut|Vec<u8>|Arc<\[u8\]>)|pub [a-zA-Z_][a-zA-Z0-9_]*\s*:\s*(Vec<u8>|Arc<\[u8\]>|&mut|\*mut)' crates/muxiva-types/src; then exit 1; fi
```

Expected GREEN: both scans return no matches. If the public-field expression
matches an enum variant payload rather than a struct field, narrow the command
to `pub struct` bodies and record the reason in the report; do not suppress a
real mutable/public storage exposure.

- [ ] **Step 5 — run document/repository hygiene**

```bash
if rg -n '[T]ODO|[T]BD|[F]IXME' README.md docs crates; then exit 1; fi
git diff --check
git status --short
```

Expected GREEN: placeholder and diff checks produce no output. Before the
report commit, status lists only the intended Stage 3 report or other reviewed
Stage 3 files from the current task; no unrelated path is staged.

- [ ] **Step 6 — commit the report**

```bash
git add docs/pre_release_notes/03-frames-and-ownership.md
git commit -m "docs: report Stage 3 frames and ownership"
```

- [ ] **Step 7 — final clean-tree gate**

```bash
git status --short
git log --oneline --decorate -12
```

Expected GREEN: status is empty and the Stage 3 commits are visible. Stop.
Do not create a Node, Edge, GraphBuilder, lifecycle hook, queue, runtime,
binding, or Stage 4 implementation.

## Review checklist

- [ ] New IDs are nominally distinct and existing IDs are unchanged.
- [ ] All live data is owned; FrameBuffer exposes immutable bytes only.
- [ ] Copy/Retain/Release are documentation, not foreign-buffer code.
- [ ] Audio/video expected lengths and offsets use checked arithmetic.
- [ ] Invalid audio indices use `MUXIVA-FRM-AUDIO-PLANE`; foreign VideoPlane
  references use `MUXIVA-FRM-VIDEO-PLANE`.
- [ ] Bare Timestamp ordering does not compile and header comparison requires
  complete clock-domain equality.
- [ ] Six Frame variants match six FrameType values exactly.
- [ ] Signal and Event are Frames and introduce no routing side channel.
- [ ] Unknown extensions survive default derivation byte/value-for-value.
- [ ] Private extensions are readable to receivers but absent from public,
  log-safe, and direct Frame/FrameHeader/Extension Debug views.
- [ ] Derivation appends one parent/origin/reason entry and cannot mutate the
  parent.
- [ ] No live Frame serde/JSON surface or forbidden dependency exists.
- [ ] The example constructs/derives Frames only.
- [ ] The timestamp breaking correction is reported with migration guidance;
  all remaining Stage 2 deferred findings stay visible and unclaimed.
- [ ] The final report and gates stop at Stage 3 acceptance.
