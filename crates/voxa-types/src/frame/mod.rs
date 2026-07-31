//! Immutable frame header and payload value types.

use std::fmt;

use crate::{
    ErrorCategory, Extension, Extensions, FrameId, LineageEntry, Metadata, Result, SequenceId,
    StreamId, Timestamp, TraceId, TransformOrigin, VoxaError,
};

mod audio;
mod header;
mod message;
mod video;

pub use audio::{AudioData, AudioLayout, PcmSampleFormat};
pub use header::{ClockDomain, ClockKind, FrameHeader};
pub use message::{ByteData, EventData, MediaType, SignalData, TextData};
pub use video::{PixelFormat, VideoData, VideoLayout, VideoPlane};

pub(super) fn checked_size_product(left: usize, right: usize) -> crate::Result<usize> {
    left.checked_mul(right).ok_or_else(arithmetic_error)
}

pub(super) fn checked_size_sum(left: usize, right: usize) -> crate::Result<usize> {
    left.checked_add(right).ok_or_else(arithmetic_error)
}

fn arithmetic_error() -> crate::VoxaError {
    crate::VoxaError::new(
        crate::ErrorCategory::Validation,
        "VOXA-FRM-ARITHMETIC",
        "frame size arithmetic overflowed",
    )
}

/// Identifies the payload variant carried by a frame.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FrameType {
    /// PCM audio samples.
    Audio,
    /// Pixel video data.
    Video,
    /// UTF-8 text.
    Text,
    /// Opaque bytes.
    Byte,
    /// A graph-local signal.
    Signal,
    /// A published event.
    Event,
}

/// One of the six validated payload types accepted by [`Frame::new`].
#[derive(Clone, Eq, PartialEq)]
pub enum FramePayload {
    /// PCM audio samples.
    Audio(AudioData),
    /// Pixel video data.
    Video(VideoData),
    /// UTF-8 text.
    Text(TextData),
    /// Opaque bytes.
    Byte(ByteData),
    /// A graph-local signal value.
    Signal(SignalData),
    /// A published event value.
    Event(EventData),
}

/// Consuming configuration for deriving one immutable frame from another.
pub struct FrameDerivation {
    new_frame_id: FrameId,
    timestamp: Timestamp,
    sequence_id: SequenceId,
    origin: TransformOrigin,
    reason: Box<str>,
    metadata: Option<Metadata>,
    extensions: Option<Extensions>,
    payload: Option<FramePayload>,
}

impl FrameDerivation {
    /// Creates a derivation that preserves metadata, extensions, and payload.
    pub fn new(
        new_frame_id: FrameId,
        timestamp: Timestamp,
        sequence_id: SequenceId,
        origin: TransformOrigin,
        reason: impl Into<Box<str>>,
    ) -> Result<Self> {
        let reason = reason.into();
        LineageEntry::validate_reason(&reason)?;

        Ok(Self {
            new_frame_id,
            timestamp,
            sequence_id,
            origin,
            reason,
            metadata: None,
            extensions: None,
            payload: None,
        })
    }

    /// Replaces the parent's metadata in the derived frame.
    pub fn with_metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Replaces the parent's complete extension collection in the derived frame.
    pub fn with_extensions(mut self, extensions: Extensions) -> Self {
        self.extensions = Some(extensions);
        self
    }

    /// Replaces the parent's payload and selects the resulting frame type from it.
    pub fn with_payload(mut self, payload: FramePayload) -> Self {
        self.payload = Some(payload);
        self
    }
}

impl FramePayload {
    /// Returns the payload's frame type.
    pub const fn frame_type(&self) -> FrameType {
        match self {
            Self::Audio(_) => FrameType::Audio,
            Self::Video(_) => FrameType::Video,
            Self::Text(_) => FrameType::Text,
            Self::Byte(_) => FrameType::Byte,
            Self::Signal(_) => FrameType::Signal,
            Self::Event(_) => FrameType::Event,
        }
    }
}

macro_rules! concrete_frame {
    ($name:ident, $data:ty) => {
        #[doc = concat!("An immutable concrete `", stringify!($name), "` wrapper.")]
        #[derive(Clone, Eq, PartialEq)]
        pub struct $name {
            header: FrameHeader,
            data: $data,
        }

        impl $name {
            /// Returns the common immutable frame header.
            pub fn header(&self) -> &FrameHeader {
                &self.header
            }

            /// Returns the typed immutable frame payload.
            pub fn data(&self) -> &$data {
                &self.data
            }
        }
    };
}

concrete_frame!(AudioFrame, AudioData);
concrete_frame!(VideoFrame, VideoData);
concrete_frame!(TextFrame, TextData);
concrete_frame!(ByteFrame, ByteData);
concrete_frame!(SignalFrame, SignalData);
concrete_frame!(EventFrame, EventData);

/// An immutable frame with exactly one typed payload.
#[derive(Clone, Eq, PartialEq)]
pub enum Frame {
    /// An audio frame.
    Audio(AudioFrame),
    /// A video frame.
    Video(VideoFrame),
    /// A text frame.
    Text(TextFrame),
    /// An opaque byte frame.
    Byte(ByteFrame),
    /// A signal frame.
    Signal(SignalFrame),
    /// An event frame.
    Event(EventFrame),
}

impl Frame {
    /// Assembles a frame after checking that header and payload types match.
    pub fn new(header: FrameHeader, payload: FramePayload) -> Result<Self> {
        if header.frame_type() != payload.frame_type() {
            return Err(type_mismatch_error(
                header.frame_type(),
                payload.frame_type(),
            ));
        }
        Ok(assemble_frame(header, payload))
    }

    /// Returns the frame's payload type.
    pub const fn frame_type(&self) -> FrameType {
        match self {
            Self::Audio(_) => FrameType::Audio,
            Self::Video(_) => FrameType::Video,
            Self::Text(_) => FrameType::Text,
            Self::Byte(_) => FrameType::Byte,
            Self::Signal(_) => FrameType::Signal,
            Self::Event(_) => FrameType::Event,
        }
    }

    /// Returns the common immutable header.
    pub fn header(&self) -> &FrameHeader {
        match self {
            Self::Audio(frame) => frame.header(),
            Self::Video(frame) => frame.header(),
            Self::Text(frame) => frame.header(),
            Self::Byte(frame) => frame.header(),
            Self::Signal(frame) => frame.header(),
            Self::Event(frame) => frame.header(),
        }
    }

    /// Returns the audio wrapper when this is an audio frame.
    pub fn as_audio(&self) -> Option<&AudioFrame> {
        match self {
            Self::Audio(frame) => Some(frame),
            _ => None,
        }
    }

    /// Returns the video wrapper when this is a video frame.
    pub fn as_video(&self) -> Option<&VideoFrame> {
        match self {
            Self::Video(frame) => Some(frame),
            _ => None,
        }
    }

    /// Returns the text wrapper when this is a text frame.
    pub fn as_text(&self) -> Option<&TextFrame> {
        match self {
            Self::Text(frame) => Some(frame),
            _ => None,
        }
    }

    /// Returns the byte wrapper when this is an opaque byte frame.
    pub fn as_byte(&self) -> Option<&ByteFrame> {
        match self {
            Self::Byte(frame) => Some(frame),
            _ => None,
        }
    }

    /// Returns the signal wrapper when this is a signal frame.
    pub fn as_signal(&self) -> Option<&SignalFrame> {
        match self {
            Self::Signal(frame) => Some(frame),
            _ => None,
        }
    }

    /// Returns the event wrapper when this is an event frame.
    pub fn as_event(&self) -> Option<&EventFrame> {
        match self {
            Self::Event(frame) => Some(frame),
            _ => None,
        }
    }

    /// Validates the frame type expected by a typed consumer.
    pub fn ensure_type(&self, expected: FrameType) -> Result<()> {
        let actual = self.frame_type();
        if actual != expected {
            return Err(type_mismatch_error(expected, actual));
        }
        Ok(())
    }

    /// Derives a new immutable frame while preserving unspecified parent values.
    pub fn derive(&self, derivation: FrameDerivation) -> Result<Self> {
        if self.header().frame_id() == &derivation.new_frame_id {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-DERIVATION-ID",
                "a derived frame must use a different ID from its direct parent",
            ));
        }

        let FrameDerivation {
            new_frame_id,
            timestamp,
            sequence_id,
            origin,
            reason,
            metadata,
            extensions,
            payload,
        } = derivation;
        let parent_header = self.header();
        let payload = payload.unwrap_or_else(|| self.cloned_payload());
        let lineage = parent_header.lineage().clone().append(LineageEntry::new(
            parent_header.frame_id().clone(),
            origin,
            reason,
        )?);
        let header = FrameHeader::new(
            new_frame_id,
            timestamp,
            parent_header.clock_domain().clone(),
            sequence_id,
            parent_header.stream_id().clone(),
            parent_header.trace_id().clone(),
            payload.frame_type(),
            metadata.unwrap_or_else(|| parent_header.metadata().clone()),
            extensions.unwrap_or_else(|| parent_header.extensions().clone()),
            lineage,
        )?;

        Self::new(header, payload)
    }

    /// Attaches this frame as the direct parent of an immutable replacement.
    ///
    /// This is the narrow runtime bridge used when an Edge policy supplies a
    /// complete replacement frame. The replacement's identity, timing,
    /// routing, metadata, extensions, and payload are retained, while its
    /// caller-supplied lineage is replaced with this parent's lineage plus one
    /// validated entry. This prevents a policy from forging or duplicating the
    /// automatic Edge lineage entry.
    pub fn attach_replacement_lineage(
        &self,
        replacement: Frame,
        origin: TransformOrigin,
        reason: impl Into<Box<str>>,
    ) -> Result<Self> {
        if self.header().frame_id() == replacement.header().frame_id() {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-REPLACEMENT-ID",
                "a replacement frame must use a different ID from its direct parent",
            ));
        }

        let replacement_header = replacement.header();
        let lineage = self.header().lineage().clone().append(LineageEntry::new(
            self.header().frame_id().clone(),
            origin,
            reason,
        )?);
        let header = FrameHeader::new(
            replacement_header.frame_id().clone(),
            replacement_header.timestamp(),
            replacement_header.clock_domain().clone(),
            replacement_header.sequence_id(),
            replacement_header.stream_id().clone(),
            replacement_header.trace_id().clone(),
            replacement.frame_type(),
            replacement_header.metadata().clone(),
            replacement_header.extensions().clone(),
            lineage,
        )?;

        Self::new(header, replacement.cloned_payload())
    }

    /// Returns a borrowed view that filters private extensions.
    pub fn public_view(&self) -> PublicFrameView<'_> {
        PublicFrameView { frame: self }
    }

    /// Returns a borrowed view containing only bounded diagnostic fields.
    pub fn log_safe_view(&self) -> LogSafeFrameView<'_> {
        LogSafeFrameView {
            frame: self,
            header: self.header(),
        }
    }

    fn cloned_payload(&self) -> FramePayload {
        match self {
            Self::Audio(frame) => FramePayload::Audio(frame.data().clone()),
            Self::Video(frame) => FramePayload::Video(frame.data().clone()),
            Self::Text(frame) => FramePayload::Text(frame.data().clone()),
            Self::Byte(frame) => FramePayload::Byte(frame.data().clone()),
            Self::Signal(frame) => FramePayload::Signal(frame.data().clone()),
            Self::Event(frame) => FramePayload::Event(frame.data().clone()),
        }
    }

    fn payload_byte_len(&self) -> Option<usize> {
        match self {
            Self::Audio(frame) => Some(frame.data().buffer().len()),
            Self::Video(frame) => Some(frame.data().buffer().len()),
            Self::Text(frame) => Some(frame.data().as_str().len()),
            Self::Byte(frame) => Some(frame.data().buffer().len()),
            Self::Signal(_) | Self::Event(_) => None,
        }
    }
}

/// A borrowed public frame view that does not expose private extensions.
pub struct PublicFrameView<'a> {
    frame: &'a Frame,
}

impl<'a> PublicFrameView<'a> {
    /// Returns the filtered public header view.
    pub fn header(&self) -> PublicFrameHeaderView<'a> {
        PublicFrameHeaderView {
            header: self.frame.header(),
        }
    }

    /// Returns the frame's payload type.
    pub const fn frame_type(&self) -> FrameType {
        self.frame.frame_type()
    }
}

/// A borrowed header view that exposes only public extensions.
pub struct PublicFrameHeaderView<'a> {
    header: &'a FrameHeader,
}

impl<'a> PublicFrameHeaderView<'a> {
    /// Returns the frame identity.
    pub fn frame_id(&self) -> &'a FrameId {
        self.header.frame_id()
    }

    /// Returns the timestamp scalar interpreted by the clock domain.
    pub const fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Returns the timestamp's clock domain.
    pub fn clock_domain(&self) -> &'a ClockDomain {
        self.header.clock_domain()
    }

    /// Returns the sequence counter within the stream.
    pub const fn sequence_id(&self) -> SequenceId {
        self.header.sequence_id()
    }

    /// Returns the stream identity.
    pub fn stream_id(&self) -> &'a StreamId {
        self.header.stream_id()
    }

    /// Returns the trace identity.
    pub fn trace_id(&self) -> &'a TraceId {
        self.header.trace_id()
    }

    /// Returns immutable frame metadata.
    pub fn metadata(&self) -> &'a Metadata {
        self.header.metadata()
    }

    /// Iterates over public extensions in input order.
    pub fn extensions(&self) -> impl Iterator<Item = &'a Extension> {
        self.header.extensions().public_iter()
    }

    /// Returns immutable transformation history.
    pub fn lineage(&self) -> &'a crate::Lineage {
        self.header.lineage()
    }
}

/// A borrowed frame view containing only log-safe scalar diagnostics.
pub struct LogSafeFrameView<'a> {
    frame: &'a Frame,
    header: &'a FrameHeader,
}

impl<'a> LogSafeFrameView<'a> {
    /// Returns the frame identity.
    pub fn frame_id(&self) -> &'a FrameId {
        self.header.frame_id()
    }

    /// Returns the stream identity.
    pub fn stream_id(&self) -> &'a StreamId {
        self.header.stream_id()
    }

    /// Returns the trace identity.
    pub fn trace_id(&self) -> &'a TraceId {
        self.header.trace_id()
    }

    /// Returns the frame's payload type.
    pub const fn frame_type(&self) -> FrameType {
        self.frame.frame_type()
    }

    /// Returns the timestamp scalar interpreted by the clock domain.
    pub const fn timestamp(&self) -> Timestamp {
        self.header.timestamp()
    }

    /// Returns the timestamp's clock domain.
    pub fn clock_domain(&self) -> &'a ClockDomain {
        self.header.clock_domain()
    }

    /// Returns the sequence counter within the stream.
    pub const fn sequence_id(&self) -> SequenceId {
        self.header.sequence_id()
    }

    /// Returns a cheaply known payload byte length, when one exists.
    pub fn payload_byte_len(&self) -> Option<usize> {
        self.frame.payload_byte_len()
    }

    /// Returns the number of metadata keys without exposing keys or values.
    pub fn metadata_key_count(&self) -> usize {
        self.header.metadata().len()
    }

    /// Returns the number of public extensions without exposing private records.
    pub fn public_extension_count(&self) -> usize {
        self.header.extensions().public_iter().count()
    }

    /// Returns the number of lineage entries without exposing reasons.
    pub fn lineage_count(&self) -> usize {
        self.header.lineage().len()
    }
}

fn assemble_frame(header: FrameHeader, payload: FramePayload) -> Frame {
    match payload {
        FramePayload::Audio(data) => Frame::Audio(AudioFrame { header, data }),
        FramePayload::Video(data) => Frame::Video(VideoFrame { header, data }),
        FramePayload::Text(data) => Frame::Text(TextFrame { header, data }),
        FramePayload::Byte(data) => Frame::Byte(ByteFrame { header, data }),
        FramePayload::Signal(data) => Frame::Signal(SignalFrame { header, data }),
        FramePayload::Event(data) => Frame::Event(EventFrame { header, data }),
    }
}

fn type_mismatch_error(expected: FrameType, actual: FrameType) -> VoxaError {
    VoxaError::new(
        ErrorCategory::Validation,
        "VOXA-FRM-TYPE-MISMATCH",
        "frame header, payload, or expected type differs",
    )
    .with_context("expected_frame_type", format!("{expected:?}"))
    .with_context("actual_frame_type", format!("{actual:?}"))
}

impl fmt::Debug for Frame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let view = self.log_safe_view();
        formatter
            .debug_struct("Frame")
            .field("frame_id", view.frame_id())
            .field("stream_id", view.stream_id())
            .field("trace_id", view.trace_id())
            .field("frame_type", &view.frame_type())
            .field("timestamp", &view.timestamp())
            .field("clock_domain", view.clock_domain())
            .field("sequence_id", &view.sequence_id())
            .field("payload_byte_len", &view.payload_byte_len())
            .field("metadata_key_count", &view.metadata_key_count())
            .field("public_extension_count", &view.public_extension_count())
            .field("lineage_count", &view.lineage_count())
            .finish()
    }
}
