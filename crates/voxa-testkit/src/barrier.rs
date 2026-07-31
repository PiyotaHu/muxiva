use std::{
    fmt,
    sync::{Condvar, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateError {
    label: Box<str>,
    expected: usize,
    arrived: usize,
}

impl fmt::Display for GateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "gate '{}' timed out: {}/{} participants arrived",
            self.label, self.arrived, self.expected
        )
    }
}

impl std::error::Error for GateError {}

#[derive(Debug)]
struct GateState {
    arrived: usize,
    released: bool,
}

/// A named two-phase rendezvous for deterministic concurrency tests.
#[derive(Debug)]
pub struct TestGate {
    label: Box<str>,
    expected: usize,
    state: Mutex<GateState>,
    changed: Condvar,
}

impl TestGate {
    pub fn new(label: impl Into<Box<str>>, expected: usize) -> Self {
        assert!(expected > 0, "a gate needs at least one participant");
        Self {
            label: label.into(),
            expected,
            state: Mutex::new(GateState {
                arrived: 0,
                released: false,
            }),
            changed: Condvar::new(),
        }
    }

    /// Records arrival and waits for the controller to release the gate.
    pub fn arrive_and_wait(&self, timeout: Duration) -> Result<(), GateError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.arrived = state.arrived.saturating_add(1);
        self.changed.notify_all();
        while !state.released {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(self.error(&state));
            }
            let (next, result) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
            if result.timed_out() && !state.released {
                return Err(self.error(&state));
            }
        }
        Ok(())
    }

    /// Waits until every expected participant has reached the gate.
    pub fn wait_until_arrived(&self, timeout: Duration) -> Result<(), GateError> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        while state.arrived < self.expected {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(self.error(&state));
            }
            let (next, result) = self
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
            if result.timed_out() && state.arrived < self.expected {
                return Err(self.error(&state));
            }
        }
        Ok(())
    }

    pub fn release(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.released = true;
        self.changed.notify_all();
    }

    pub fn arrived(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .arrived
    }

    fn error(&self, state: &GateState) -> GateError {
        GateError {
            label: self.label.clone(),
            expected: self.expected,
            arrived: state.arrived,
        }
    }
}
