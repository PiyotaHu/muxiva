#![forbid(unsafe_code)]
//! Dependency-light public value and error types for Voxa.

mod error;
mod id;
mod schema;
mod time;

pub use error::{ErrorCategory, ErrorCodeError, ErrorContext, Result, VoxaError};
pub use id::{
    ClockDomainId, EdgeId, FrameId, IdentifierError, NodeId, ProducerId, SessionId, StreamId,
    TraceId,
};
pub use schema::{NamespacedName, SchemaVersion};
pub use time::{SequenceId, Timestamp};
