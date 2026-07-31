#![forbid(unsafe_code)]
//! Dependency-light public value and error types for Voxa.

mod error;
mod frame_buffer;
mod id;
mod schema;
mod time;
mod value;

pub use error::{ErrorCategory, ErrorCodeError, ErrorContext, Result, VoxaError};
pub use frame_buffer::FrameBuffer;
pub use id::{
    ClockDomainId, EdgeId, FrameId, IdentifierError, NodeId, ProducerId, SessionId, StreamId,
    TraceId,
};
pub use schema::{NamespacedName, SchemaVersion};
pub use time::{SequenceId, Timestamp};
pub use value::{FiniteF64, Metadata, Value, ValueMap};
