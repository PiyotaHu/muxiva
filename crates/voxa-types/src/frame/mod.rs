//! Immutable frame header and payload value types.

mod audio;
mod header;
mod video;

pub use audio::{AudioData, AudioLayout, PcmSampleFormat};
pub use header::{ClockDomain, ClockKind, FrameHeader};
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
