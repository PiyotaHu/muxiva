#![forbid(unsafe_code)]
//! Dependency-light public value and error types for Voxa.

mod error;
mod extension;
mod frame;
mod frame_buffer;
mod id;
mod lineage;
mod schema;
mod time;
mod value;

pub use error::{ErrorCategory, ErrorCodeError, ErrorContext, Result, VoxaError};
pub use extension::{Extension, ExtensionProducer, ExtensionVisibility, Extensions};
pub use frame::{
    AudioData, AudioLayout, ClockDomain, ClockKind, FrameHeader, FrameType, PcmSampleFormat,
    PixelFormat, VideoData, VideoLayout, VideoPlane,
};
pub use frame_buffer::FrameBuffer;
pub use id::{
    ClockDomainId, EdgeId, FrameId, IdentifierError, NodeId, ProducerId, SessionId, StreamId,
    TraceId,
};
pub use lineage::{Lineage, LineageEntry, TransformOrigin};
pub use schema::{NamespacedName, SchemaVersion};
pub use time::{SequenceId, Timestamp};
pub use value::{FiniteF64, Metadata, Value, ValueMap};
