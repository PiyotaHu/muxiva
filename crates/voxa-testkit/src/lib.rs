//! Deterministic, dependency-light fixtures for Voxa contract tests.
//!
//! This crate is workspace-internal and must never become a production
//! dependency. Its synchronization helpers use explicit gates and monotonic
//! deadlines so race tests do not depend on arbitrary sleeps.

mod barrier;
mod clock;
mod leak;
mod thread;

pub use barrier::{GateError, TestGate};
pub use clock::ManualClock;
pub use leak::{LeakProbe, LeakSnapshot};
pub use thread::{ThreadEvent, ThreadProbe};
