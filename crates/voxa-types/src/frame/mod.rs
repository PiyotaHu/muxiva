//! Immutable frame header and payload value types.

use std::fmt;

use crate::{ErrorCategory, Result, VoxaError};

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
        let header = self.header();
        formatter
            .debug_struct("Frame")
            .field("frame_id", header.frame_id())
            .field("stream_id", header.stream_id())
            .field("trace_id", header.trace_id())
            .field("frame_type", &self.frame_type())
            .field("timestamp", &header.timestamp())
            .field("clock_domain", header.clock_domain())
            .field("sequence_id", &header.sequence_id())
            .field("payload_byte_len", &self.payload_byte_len())
            .field("metadata_key_count", &header.metadata().len())
            .field(
                "public_extension_count",
                &header.extensions().public_iter().count(),
            )
            .field("lineage_count", &header.lineage().len())
            .finish()
    }
}
