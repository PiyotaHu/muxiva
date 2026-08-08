# Muxiva Stage 3 Frame and Ownership Contract

Status: **Approved implementation contract; production code not yet written**

Contract version: **0.1.0-draft.1**

Last updated: **2026-08-01**

## 1. Purpose and authority

This document fixes the Stage 3 Rust data model before concurrency, graph
execution, and foreign-language boundaries exist. It refines the Stage 1 Frame
and ownership clauses without replacing them. If this document and the Stage 1
contract appear to differ, the narrower Stage 3 rule applies only to the
surfaces named here; all other Stage 1 requirements remain normative.

Stage 3 makes a `Frame` the only value that a later Node, Edge, Signal route,
or EventBus may transport. It defines immutable Rust ownership, exact media
layouts, validation, privacy-aware diagnostic views, and the future FFI
ownership modes. It does not provide a Runtime that transports a Frame.

## 2. Scope boundary

Stage 3 adds only dependency-light types in `muxiva-types`, a construction and
derivation example in `muxiva-examples`, tests, and the Stage 3 report. It adds:

- strong `FrameId`, `EdgeId`, `ClockDomainId`, and `ProducerId` types without
  making any existing ID interchangeable;
- `Value`, immutable metadata, versioned public/private extensions, lineage,
  clock domains, `FrameHeader`, `FrameBuffer`, and six Frame variants;
- PCM audio and RGBA8/YUV420P video layout validation;
- explicit public-diagnostic and default-log-safe borrowed views;
- checked construction and exact-frame-type validation for later Edge use;
  and
- a pre-1.0 correction that removes raw ordering from `Timestamp` and requires
  clock-domain-checked header comparison.

Stage 3 does **not** add `unsafe`, Tokio or another async runtime, graph
execution, Nodes, Edges, queues, scheduling, FFI implementation, a C header,
serde, JSON serialization of live Frames, media SDKs, RTC, FFmpeg, Python,
Node-API, or C++. `EdgeId` and type-gate helpers are data needed by future
stages; they do not create an Edge or a graph.

No live `Frame`, `FrameHeader`, `FrameBuffer`, `Value`, metadata, extension, or
lineage type implements `serde::Serialize` or `serde::Deserialize` in Stage 3.
A later graph schema or explicit export DTO must not obtain serialization by
deriving it on live Frames.

## 3. Module ownership

The implementation is split by concern:

```text
crates/muxiva-types/src/
  id.rs                 Existing IDs plus FrameId, EdgeId, ClockDomainId, ProducerId
  time.rs               Timestamp ordering correction; checked scalar arithmetic
  schema.rs             SchemaVersion and namespaced-name validation
  frame_buffer.rs       Arc-backed immutable bytes
  value.rs              Cross-language-owned Value and Metadata
  extension.rs          Versioned extension records and visibility
  lineage.rs            TransformOrigin, LineageEntry, and Lineage
  frame/
    mod.rs              FrameType, FramePayload, Frame, derivation, views
    header.rs           ClockDomain and FrameHeader
    audio.rs            PCM sample format and audio layout
    video.rs            Pixel format, planes, and video layout
    message.rs          Text, byte, signal, and event payloads
```

`muxiva-types/src/lib.rs` is an export surface, not an implementation module.
The crate remains `#![forbid(unsafe_code)]` and gains no dependency beyond the
Stage 2 dependency set.

## 4. Identity, schema, and namespace values

`FrameId`, `EdgeId`, `ClockDomainId`, and `ProducerId` use the exact existing
ID representation and validation contract: private `Box<str>`, 1 through 255
UTF-8 bytes, no leading or trailing whitespace, and no ASCII control
characters. They expose `new`, `as_str`, `Display`, and `FromStr`, and derive
the same clone, comparison, ordering, and hashing traits as `NodeId`.

They are distinct nominal types. In particular, a `FrameId` cannot satisfy an
`EdgeId`, `NodeId`, `StreamId`, or `TraceId` parameter. Stage 3 does not add
generic conversion to or from `String` because that would weaken separation.

`SchemaVersion` is a private `u32` newtype. `SchemaVersion::new` rejects zero
with `MUXIVA-FRM-SCHEMA-VERSION`; `get` returns the non-zero value. Stage 3 does
not parse semantic-version text for per-payload schemas.

`NamespacedName` owns a `Box<str>` and accepts 3 through 255 ASCII bytes. It
must have at least two non-empty dot-separated segments. Each segment starts
with an ASCII letter or digit and thereafter contains only ASCII letters,
digits, `_`, or `-`. It cannot end in `_` or `-`. This grammar accepts
`com.example.trace`, `team.flow_pressure`, and `muxiva.turn.interrupted`, while
rejecting hidden unqualified names. Extension keys, signal names, and event
topics use this one type.

## 5. Value and metadata

The cross-language value algebra is closed and owned:

```rust
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
```

`FiniteF64` has a private `f64` field. `FiniteF64::new(value)` rejects NaN and
positive or negative infinity with `MUXIVA-FRM-VALUE-NUMBER`; `get` returns the
finite number. This keeps equality reflexive and avoids language-dependent
non-finite number behavior. `ValueMap` privately wraps
`BTreeMap<Box<str>, Value>` and exposes `empty`, `try_from_iter`, `get`, `iter`,
`len`, and `is_empty`. Keys may be any non-empty string of at most 255 UTF-8
bytes without ASCII controls; `try_from_iter` validates every key with
`MUXIVA-FRM-VALUE-KEY`. Duplicate input keys return the same code rather than
silently replacing an earlier value. `Value::Map(ValueMap)` is therefore not
an unchecked construction path. The ordered map makes diagnostics
deterministic; it does not imply JSON support.

`Metadata` is a private `BTreeMap<Box<str>, Value>`. `Metadata::empty` and
`Metadata::try_from_iter` construct it; keys use the same rules as Value maps.
It exposes only `get`, `iter`, `len`, and `is_empty`. Cloning Metadata produces
an independent immutable map whose contained `FrameBuffer` values remain
read-only and Arc-shared.

There is no mutable map accessor and no interior mutability. Values may be
read by a receiving Node in later stages. Metadata is not automatically safe
to log merely because it is structurally valid.

## 6. Extensions and privacy

An extension is exact immutable data:

```rust
pub enum ExtensionVisibility { Public, Private }

pub enum ExtensionProducer {
    Core,
    Node(NodeId),
    External(ProducerId),
}

pub struct Extension {
    key: NamespacedName,
    schema_version: SchemaVersion,
    producer: ExtensionProducer,
    visibility: ExtensionVisibility,
    value: Value,
}
```

All fields have borrowed accessors. `Extensions` stores `Box<[Extension]>` in
input order. `Extensions::try_from_iter` rejects a duplicate `(key,
schema_version)` pair with `MUXIVA-FRM-EXTENSION-DUPLICATE`; multiple schema
versions of one key may coexist during migration. `get` therefore takes both
the key and schema version. `iter` exposes all records to a receiving Node,
while `public_iter` exposes only records marked `Public`.

`Private` means excluded from `Frame::public_view`,
`Frame::log_safe_view`, and default `Debug` output. It is not encryption,
authorization, redaction at rest, or a promise that receiving Nodes cannot
read the value. Callers holding a Frame can use `header().extensions().iter()`
and inspect private records. Code needing an access-control boundary must not
put a secret into a Frame that an untrusted Node receives.

`Extension` has a hand-written Debug implementation. A public record prints
its key, schema version, producer, and visibility but omits its Value. A
private record prints `"<private>"` in place of its key and also omits its
Value. Direct formatting of an Extension therefore follows the same default
boundary as formatting a Frame or FrameHeader.

Unknown keys and schema versions are ordinary `Extension` values. Derivation
copies the complete `Extensions` collection unchanged unless the caller
explicitly supplies a replacement collection. Recognition by Core is never a
condition for preservation.

## 7. Lineage

Lineage records transformations, never payload snapshots:

```rust
pub struct TransformOrigin {
    node_id: Option<NodeId>,
    edge_id: Option<EdgeId>,
}

pub struct LineageEntry {
    parent_frame_id: FrameId,
    origin: TransformOrigin,
    reason: Box<str>,
}

pub struct Lineage(Box<[LineageEntry]>);
```

`TransformOrigin::new` requires at least one of Node or Edge and otherwise
returns `MUXIVA-FRM-LINEAGE-ORIGIN`. This supports Node transformations, future
Edge policy replacements, and a replacement attributed to both.
`LineageEntry::new` accepts a non-empty reason of at most 256 UTF-8 bytes and
rejects ASCII controls with `MUXIVA-FRM-LINEAGE-REASON`. Reasons identify an
operation such as `normalize-volume`; they must not contain transcripts,
credentials, media bytes, extension values, or other private payload data.

`Lineage::empty`, `iter`, `len`, and `is_empty` are public. Only the crate's
derivation path appends an entry. A new source Frame may have empty lineage.
`FrameHeader::new` rejects a lineage entry whose parent equals the new
`frame_id` with `MUXIVA-FRM-LINEAGE-CYCLE`. This local check does not claim to
prove ancestry across a distributed system.

## 8. Clock domains and common header

`ClockKind` has exactly `Monotonic`, `MediaRelative`, and `WallClock` variants.
`ClockDomain` is the pair of a `ClockDomainId` and a `ClockKind`. Two
timestamps are comparable for ordering only when their entire `ClockDomain`
values are equal. Equal clock kinds with different domain IDs are not an
ordering guarantee.

Stage 2 derived `Ord` and `PartialOrd` for `Timestamp`, which makes invalid
cross-domain ordering compile. Stage 3 removes those two traits as an explicit
pre-1.0 breaking correction. `Timestamp` continues to derive `Clone`, `Copy`,
`Debug`, `Eq`, `Hash`, and `PartialEq`, and retains `from_nanos`, `as_nanos`,
and `checked_add`; it has no raw ordering operator or `cmp` method. Its API
documentation is corrected from “media-relative timestamp” to “signed
nanosecond value interpreted within an explicit clock domain.” Callers that
previously used `<`, `>`, sorting, `min`, or `max` on bare timestamps must
compare the headers that supply their domains.

The header shape is:

```rust
pub struct FrameHeader {
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
}
```

`FrameHeader::new` takes those fields in that order and returns `Result<Self>`.
All fields are private and exposed only through immutable accessors. The
header has no setter, mutable borrow, public field, or interior-mutability
escape hatch. Its `Debug` implementation is log-safe and omits metadata
values, all extension values, all private-extension keys, and lineage reasons.

```rust
pub fn compare_timestamp(
    &self,
    other: &FrameHeader,
) -> Result<std::cmp::Ordering>;
```

`FrameHeader::compare_timestamp` first compares complete `ClockDomain`
values for equality. Different domains return `MUXIVA-FRM-CLOCK-DOMAIN`, even
when their `ClockKind` values match; equal domains compare the two signed
nanosecond scalars and return `Ordering`. This method provides ordering only,
not clock conversion or synchronization.

`sequence_id` is monotonic within `stream_id`; source construction owns that
policy. Stage 3 stores and exposes it but has no stream registry with which to
verify a prior sequence.

## 9. FrameBuffer ownership

The Stage 3 Rust buffer is:

```rust
#[derive(Clone)]
pub struct FrameBuffer(Arc<[u8]>);
```

`FrameBuffer::from_vec`, `FrameBuffer::from_boxed_slice`, `as_slice`, `len`,
and `is_empty` are the complete public Stage 3 surface. The conversion moves
owned bytes into Rust-managed reference-counted storage. Clone increments the
strong count; it does not copy bytes. Dropping the last clone releases the
allocation on the dropping thread. `DerefMut`, mutable pointer access,
`AsMut`, and `Arc::get_mut` are not exposed. `Debug` prints length only.

`Arc<[u8]>` is `Send + Sync`, so multiple threads may clone and read one
buffer concurrently. This is an ownership guarantee, not a queue or worker
implementation. Stage 3 tests use an internal `Weak<[u8]>` observation to
prove that the allocation remains alive until the last clone and is then
released; reference counts are not public API.

### 9.1 Future C/C++ modes, design only

Stage 3 documents but does not implement two future C ABI input modes:

1. **Copy (default).** The Adapter validates pointer and length and copies into
   a Rust-owned `FrameBuffer` before the SDK callback returns. Copy is required
   whenever the SDK has not explicitly promised that the bytes may outlive
   the callback. A borrowed pointer never enters a queue or survives the call.
2. **Retain/Release (optional capability).** The Adapter may describe retain
   and release callbacks only when the SDK explicitly supports retaining the
   object for the required lifetime. The future safe Rust wrapper retains
   before accepting the Frame and releases exactly once after the last Core
   reference. If release must run on the original callback thread or another
   SDK-owned executor, the wrapper posts a release command back to the Adapter;
   Rust does not invoke release directly on its dropping thread.

The Stage 1 wording that Retain/Release is allowed only for thread-safe
retain/release is interpreted strictly: a release with thread affinity is not
called from arbitrary Rust threads. It is allowed only through a future
Adapter-owned posting mechanism whose enqueue and shutdown semantics are
specified before FFI implementation. Stage 3's `FrameBuffer` contains no
foreign pointer, callback, opaque handle, or release queue.

## 10. Frame payloads and variants

`FrameType` has exactly `Audio`, `Video`, `Text`, `Byte`, `Signal`, and `Event`.
`FramePayload` and `Frame` have matching six variants. A concrete Frame owns a
header and one validated payload; fields remain private.

```rust
pub enum FramePayload {
    Audio(AudioData),
    Video(VideoData),
    Text(TextData),
    Byte(ByteData),
    Signal(SignalData),
    Event(EventData),
}

pub enum Frame {
    Audio(AudioFrame),
    Video(VideoFrame),
    Text(TextFrame),
    Byte(ByteFrame),
    Signal(SignalFrame),
    Event(EventFrame),
}
```

Each concrete wrapper exposes `header()` and its typed payload accessor.
`Frame::new(header, payload)` returns the matching variant and rejects a
header/payload mismatch with `MUXIVA-FRM-TYPE-MISMATCH`. `frame_type`, `header`,
and typed `as_*` borrowed accessors are available on `Frame`.
`Frame::ensure_type(expected)` is the only Stage 3 type-gate helper. It returns
the same mismatch code and is intended for a future Edge; it performs no graph
operation.

### 10.1 Audio

`PcmSampleFormat` has `U8`, `I16Le`, `I24Le`, `I32Le`, `F32Le`, and `F64Le`.
Its `bytes_per_sample` is respectively 1, 2, 3, 4, 4, and 8.
`AudioLayout` is `Interleaved` or `Planar`. A planar payload is one contiguous
buffer containing equal channel planes in channel order; each plane contains
`samples_per_channel` samples. An interleaved payload stores all channel
samples for one time index together.

```rust
pub struct AudioData {
    buffer: FrameBuffer,
    sample_rate_hz: u32,
    channels: u16,
    sample_format: PcmSampleFormat,
    layout: AudioLayout,
    samples_per_channel: u64,
    duration_ns: u64,
}
```

`AudioData::new` takes those fields except `duration_ns`, which it computes as
`floor(samples_per_channel * 1_000_000_000 / sample_rate_hz)`. It accepts
sample rates 1 through 768,000 Hz, channels 1 through 1,024, and at least one
sample per channel. Expected payload length is exactly
`samples_per_channel * channels * bytes_per_sample`; trailing bytes are
rejected. Every multiply and duration operation uses checked integer
arithmetic before conversion to `usize`. Planar and interleaved layouts use
the same total-length rule and immutable buffer. `AudioData::plane_bytes(0)`
returns the entire interleaved buffer, and any other interleaved plane index
returns `MUXIVA-FRM-AUDIO-PLANE`. For planar layout, the valid indices are
`0..channels`, each returning its checked contiguous channel plane; an index
outside that range returns `MUXIVA-FRM-AUDIO-PLANE`.

### 10.2 Video

`PixelFormat` has exactly `Rgba8` and `Yuv420p`. The public immutable plane
descriptor is `VideoPlane { offset: usize, stride: usize, row_bytes: usize,
rows: u32 }`, with borrowed scalar accessors.

```rust
pub enum VideoLayout {
    Rgba8 { plane: VideoPlane },
    Yuv420p { y: VideoPlane, u: VideoPlane, v: VideoPlane },
}

pub struct VideoData {
    buffer: FrameBuffer,
    width: u32,
    height: u32,
    pixel_format: PixelFormat,
    layout: VideoLayout,
}
```

Callers use `VideoData::rgba8(buffer, width, height, stride)` or
`VideoData::yuv420p(buffer, width, height, y_stride, u_stride, v_stride)`.
Width and height must be non-zero. RGBA row bytes are `width * 4`; its stride
must be at least that value. YUV420P requires even width and even height; Y
row bytes are `width`, U/V row bytes are `width / 2`, Y has `height` rows, and
U/V have `height / 2` rows. Each stride must be at least its plane's row bytes.

Planes are tightly sequenced in one buffer: Y begins at zero, U begins after
`y_stride * height`, and V begins after U. RGBA begins at zero. The payload
length must exactly equal the checked sum of `stride * rows` for all planes.
This rejects hidden trailing storage and makes the layout unambiguous. A later
zero-copy foreign view may need a richer representation; Stage 3 does not
speculate by accepting arbitrary offsets.

`VideoData::plane_bytes` accepts only a descriptor reference that is one of
the actual descriptor instances borrowed from that `VideoData` value's
`layout()`. Membership uses safe reference identity, not equality of offset,
stride, row-byte, and row scalars. Passing a descriptor borrowed from another
Video Frame, even when its scalar fields describe the same range, returns
`MUXIVA-FRM-VIDEO-PLANE`. A valid own-layout reference returns its full
stride-including immutable range after checked offset arithmetic.

### 10.3 Text, byte, signal, and event

`TextData` owns `Box<str>`. `TextData::new` accepts a Rust string, and
`TextData::from_utf8(FrameBuffer)` validates bytes and returns
`MUXIVA-FRM-TEXT-UTF8` on failure. It does not retain the input buffer after
successful conversion.

`MediaType` is an optional owned type/subtype value. Its constructor accepts
1 through 127 ASCII bytes with one `/`, non-empty type and subtype, and only
ASCII alphanumeric characters plus `!#$&^_.+-`; invalid values return
`MUXIVA-FRM-MEDIA-TYPE`. `ByteData` is a `FrameBuffer` plus
`Option<MediaType>`; an empty opaque buffer is allowed.

`SignalData` owns `name: NamespacedName`, `schema_version: SchemaVersion`,
`source: NodeId`, and `payload: Value`. `EventData` has the same shape with
`topic` in place of `name`. Their timestamp, stream, trace, and sequence are
the common header values; they do not duplicate the timestamp. These are Frame
variants, not transport APIs. Stage 3 adds no `emit_signal`, `publish`, route,
subscription, bare-Value side channel, or JSON payload path.

## 11. Validation and stable errors

All construction, access, comparison, and derivation validation failures use
`ErrorCategory::Validation`, an existing `MuxivaError`, and these stable codes:

| Code | Rejected condition |
| --- | --- |
| `MUXIVA-FRM-SCHEMA-VERSION` | zero schema version |
| `MUXIVA-FRM-NAMESPACE` | invalid extension/signal/topic namespace |
| `MUXIVA-FRM-VALUE-NUMBER` | non-finite floating point value |
| `MUXIVA-FRM-VALUE-KEY` | invalid Value-map or Metadata key |
| `MUXIVA-FRM-EXTENSION-DUPLICATE` | duplicate extension key and schema version |
| `MUXIVA-FRM-LINEAGE-ORIGIN` | lineage has neither Node nor Edge source |
| `MUXIVA-FRM-LINEAGE-REASON` | invalid lineage reason |
| `MUXIVA-FRM-LINEAGE-CYCLE` | new header names itself as a parent |
| `MUXIVA-FRM-CLOCK-DOMAIN` | timestamp comparison crosses clock domains |
| `MUXIVA-FRM-TYPE-MISMATCH` | header, payload, or expected FrameType differs |
| `MUXIVA-FRM-AUDIO-RATE` | sample rate outside 1..=768,000 |
| `MUXIVA-FRM-AUDIO-CHANNELS` | channel count outside 1..=1,024 |
| `MUXIVA-FRM-AUDIO-SAMPLES` | zero samples per channel |
| `MUXIVA-FRM-AUDIO-LENGTH` | payload length differs from checked expected length |
| `MUXIVA-FRM-AUDIO-PLANE` | audio plane index is not part of the layout |
| `MUXIVA-FRM-VIDEO-DIMENSIONS` | zero dimensions or odd YUV420P dimensions |
| `MUXIVA-FRM-VIDEO-STRIDE` | a stride is smaller than its row bytes |
| `MUXIVA-FRM-VIDEO-LENGTH` | payload length differs from checked plane total |
| `MUXIVA-FRM-VIDEO-PLANE` | plane descriptor does not belong to the VideoData layout |
| `MUXIVA-FRM-ARITHMETIC` | checked size, offset, or duration arithmetic overflow |
| `MUXIVA-FRM-TEXT-UTF8` | invalid UTF-8 text bytes |
| `MUXIVA-FRM-MEDIA-TYPE` | invalid optional media type |
| `MUXIVA-FRM-DERIVATION-ID` | a derivation reuses its direct parent's FrameId |

Errors attach non-sensitive dimensions such as expected/actual length through
`with_context`; they do not attach payload bytes, text, Value contents, or
private extension data. Stage 3 does not add missing Session/Stream error
builders solely to enrich these errors.

No payload length, dimension, stride, channel, sample, plane offset, duration,
or allocation size is computed with unchecked `+`, `*`, or narrowing casts.
Validation completes before a slice range is formed. Invalid input returns a
structured error; ordinary validation never panics.

## 12. Immutability and derivation

`FrameDerivation::new(new_frame_id, timestamp, sequence_id, origin, reason)`
validates its origin and reason. It defaults to the parent's clock domain,
stream, trace, metadata, extensions, and payload. Consuming builder methods
`with_metadata`, `with_extensions`, and `with_payload` replace those values.

`Frame::derive(&self, derivation)`:

1. rejects reuse of the direct parent's FrameId;
2. clones preserved immutable values and Arc-shares preserved buffers;
3. appends exactly one `LineageEntry` naming the current Frame as parent;
4. selects the new header `frame_type` from the resulting payload;
5. validates the new header/payload pair; and
6. returns a new Frame without mutating the parent.

This path supports metadata supplementation, extension replacement, same-type
buffer replacement, and typed transformation to another Frame variant. An
explicit replacement extension collection is responsible for preserving
unknown values; the default derivation path preserves them automatically.
Neither the parent header nor its payload buffer can be modified in place.

## 13. Public and log-safe views

`Frame::public_view()` returns `PublicFrameView<'_>`. It exposes the full
immutable header except private extensions, and it exposes public extension
values. It intentionally provides no method that returns the original
`Extensions`. Payload access remains on the explicitly held Frame rather than
the public diagnostic view.

`Frame::log_safe_view()` returns `LogSafeFrameView<'_>`. It exposes Frame,
Stream, and Trace IDs, frame type, timestamp, clock domain, sequence, payload
byte length when cheaply known, metadata key count, public extension count,
and lineage count. It exposes no payload, text, Value, metadata value,
extension value, private extension key, or lineage reason. The custom `Debug`
implementations for `Frame` and `FrameHeader` delegate to this policy.

These are borrowed Rust views, not serialization formats. Private-extension
exclusion is tested against both explicit views and formatted default Debug
output. The Stage 1 phrase “default serialization” is satisfied in Stage 3 by
having no live-Frame serializer at all; any later explicit diagnostic export
must begin from `public_view` or a separately reviewed DTO.

## 14. Minimal Stage 3 example

`cargo run -p muxiva-examples --bin frames` constructs one interleaved mono
`I16Le` Audio Frame, attaches one unknown public extension and one private
extension, derives a second Audio Frame with a replacement buffer and lineage,
and prints only its log-safe summary. The example asserts that the parent is
unchanged and the unknown extension exists in the child.

The example does not define a Node, Edge, graph, queue, worker, signal route,
EventBus, SDK callback, FFI handle, or JSON representation.

## 15. Stage 2 debt carried forward

Stage 3 preserves these visible deferred Stage 2 findings except for the
timestamp correction required by the Frame clock-domain contract:

- `TracingLogSink` still accepts arbitrary non-reserved field values, so its
  broad default-log privacy boundary is not quality-clean. The Frame example
  avoids this surface and prints only `LogSafeFrameView`.
- `ErrorContext::Session` and `ErrorContext::Stream` still lack public
  `MuxivaError` builder methods.
- tracing-output capture, concurrent and pre-installed subscriber behavior,
  identifier boundary coverage, event-name grammar wording, stale
  implementation-plan logging syntax, and literal-versus-summarized
  test-result labeling remain deferred review work.

Stage 3 intentionally resolves the Stage 2 timestamp wording and removes
`Ord`/`PartialOrd`. The Stage 3 report must identify the affected public
surface, show the bare-comparison migration to
`FrameHeader::compare_timestamp`, and record this as a pre-1.0 breaking
correction rather than silently treating it as new Frame-only behavior.

Apart from the explicit Timestamp correction above, Stage 3 may add boundary
tests that also cover the new ID types but does not rewrite another Stage 2
API or its report unless a compile failure makes a narrow change unavoidable.
Any such additional change requires explicit mention in the Stage 3 report.

## 16. Acceptance boundary

Stage 3 is accepted only when:

- all six variants construct through the exact immutable model above;
- invalid sample rates, channel counts, sample counts, payload lengths,
  audio/video plane requests, dimensions, strides, UTF-8, namespaces, media
  types, and every reachable arithmetic overflow return the tabled
  `MuxivaError` code;
- bare `Timestamp` ordering does not compile, same-domain header comparison
  orders signed nanoseconds, and different clock-domain IDs reject comparison;
- clone/move, Arc sharing, last-clone release, and concurrent read tests pass;
- private extensions are absent from public/log-safe/default `Frame`,
  `FrameHeader`, and `Extension` Debug views;
- unknown extensions survive default derivation unchanged;
- derivation creates a distinct Frame, appends correct lineage, and leaves the
  parent unchanged;
- exact type-gate mismatches are structured errors;
- the `frames` example runs without graph/runtime behavior;
- dependency and source scans find no forbidden Stage 3 capability; and
- format, Clippy, all targets, doc tests, placeholder scan, and diff checks
  pass.

Passing Stage 3 authorizes planning Stage 4. It does not authorize Stage 4
Node, Edge, policy, graph, lifecycle, or runner implementation.
