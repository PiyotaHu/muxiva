//! Deterministic, dependency-light fixtures for Muxiva contract tests.
//!
//! This crate is workspace-internal and must never become a production
//! dependency. Its synchronization helpers use explicit gates and monotonic
//! deadlines so race tests do not depend on arbitrary sleeps.

mod barrier;
mod clock;
mod edge;
mod frame;
mod graph;
mod leak;
mod managed;
mod node;
mod notification_bus;
mod signal;
mod temp;
mod thread;

pub use barrier::{GateError, TestGate};
pub use clock::ManualClock;
pub use edge::{TestEdgeDisposition, TestEdgePolicy};
pub use frame::{audio_frame, event_frame, signal_frame, text_frame};
pub use graph::TestGraphBuilder;
pub use leak::{LeakProbe, LeakSnapshot};
pub use managed::{ManagedOutcome, ScriptedManagedStreamAdapter};
pub use node::{LifecycleCall, TestNode, TestSink, TestSource};
pub use notification_bus::NotificationBusProbe;
pub use signal::SignalProbe;
pub use temp::{ReservedPort, TestDirectory};
pub use thread::{ThreadEvent, ThreadProbe};
