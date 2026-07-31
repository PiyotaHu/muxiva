//! Immutable frame header and payload value types.

mod audio;
mod header;

pub use audio::{AudioData, AudioLayout, PcmSampleFormat};
pub use header::{ClockDomain, ClockKind, FrameHeader};

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
