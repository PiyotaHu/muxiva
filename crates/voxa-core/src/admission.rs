//! Bounded per-input-port processing admission.

use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdmissionSnapshot {
    pub max_in_flight: usize,
    pub in_flight: usize,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    ZeroCapacity,
    Closed,
}

#[derive(Clone)]
pub struct AdmissionSlots {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    available: Condvar,
}

struct State {
    max: usize,
    in_flight: usize,
    closed: bool,
}

/// An owned slot returned exactly once on release or drop.
#[must_use = "dropping the lease releases its admission slot"]
pub struct AdmissionLease {
    shared: Option<Arc<Shared>>,
}

impl AdmissionSlots {
    pub fn new(max_in_flight: usize) -> Result<Self, AdmissionError> {
        if max_in_flight == 0 {
            return Err(AdmissionError::ZeroCapacity);
        }
        Ok(Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    max: max_in_flight,
                    in_flight: 0,
                    closed: false,
                }),
                available: Condvar::new(),
            }),
        })
    }

    /// Acquires immediately, allowing workers to avoid dequeuing without a slot.
    pub fn try_acquire(&self) -> Result<Option<AdmissionLease>, AdmissionError> {
        let mut state = self.shared.state.lock().expect("admission state poisoned");
        if state.closed {
            return Err(AdmissionError::Closed);
        }
        if state.in_flight == state.max {
            return Ok(None);
        }
        state.in_flight += 1;
        Ok(Some(AdmissionLease {
            shared: Some(self.shared.clone()),
        }))
    }

    /// Waits without polling until one slot is available or admission closes.
    pub fn acquire(&self) -> Result<AdmissionLease, AdmissionError> {
        let mut state = self.shared.state.lock().expect("admission state poisoned");
        loop {
            if state.closed {
                return Err(AdmissionError::Closed);
            }
            if state.in_flight < state.max {
                state.in_flight += 1;
                return Ok(AdmissionLease {
                    shared: Some(self.shared.clone()),
                });
            }
            state = self
                .shared
                .available
                .wait(state)
                .expect("admission state poisoned while waiting");
        }
    }

    /// Prevents future acquisitions and wakes all waiting workers.
    pub fn close(&self) {
        let mut state = self.shared.state.lock().expect("admission state poisoned");
        state.closed = true;
        self.shared.available.notify_all();
    }

    pub fn snapshot(&self) -> AdmissionSnapshot {
        let state = self.shared.state.lock().expect("admission state poisoned");
        AdmissionSnapshot {
            max_in_flight: state.max,
            in_flight: state.in_flight,
            closed: state.closed,
        }
    }

    /// Holds a slot for the complete synchronous operation.
    pub fn with_slot<T>(&self, operation: impl FnOnce() -> T) -> Result<T, AdmissionError> {
        let lease = self.acquire()?;
        let result = operation();
        lease.release();
        Ok(result)
    }
}

impl AdmissionLease {
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        let Some(shared) = self.shared.take() else {
            return;
        };
        let mut state = shared.state.lock().expect("admission state poisoned");
        debug_assert!(state.in_flight > 0);
        state.in_flight -= 1;
        shared.available.notify_one();
    }
}

impl Drop for AdmissionLease {
    fn drop(&mut self) {
        self.release_inner();
    }
}
