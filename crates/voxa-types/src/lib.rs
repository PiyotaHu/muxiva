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
    AudioData, AudioFrame, AudioLayout, ByteData, ByteFrame, ClockDomain, ClockKind, EventData,
    EventFrame, Frame, FrameDerivation, FrameHeader, FramePayload, FrameType, LogSafeFrameView,
    MediaType, PcmSampleFormat, PixelFormat, PublicFrameHeaderView, PublicFrameView, SignalData,
    SignalFrame, TextData, TextFrame, VideoData, VideoFrame, VideoLayout, VideoPlane,
};
pub use frame_buffer::FrameBuffer;
pub use id::{
    ClockDomainId, EdgeId, FrameId, IdentifierError, NodeId, ProducerId, SessionId, StreamId,
    TraceId,
};
pub use lineage::{Lineage, LineageEntry, MediaTimeRange, TransformOrigin};
pub use schema::{NamespacedName, SchemaVersion};
pub use time::{SequenceId, Timestamp};
pub use value::{FiniteF64, Metadata, Value, ValueMap};
