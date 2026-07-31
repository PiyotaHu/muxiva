use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};

/// Cloneable graph-wide cancellation primitive.
///
/// Cancellation is idempotent, may be requested from any thread, and wakes
/// every waiter. Queue waiters are woken separately when the runtime closes
/// its Edge queues.
#[derive(Clone, Default)]
pub struct StopToken {
    inner: Arc<StopState>,
}

#[derive(Default)]
struct StopState {
    cancelled: AtomicBool,
    generation: Mutex<u64>,
    changed: Condvar,
}

impl StopToken {
    /// Creates a token in the running state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation. Returns true only for the first request.
    pub fn cancel(&self) -> bool {
        if self.inner.cancelled.swap(true, Ordering::AcqRel) {
            return false;
        }
        let mut generation = self
            .inner
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *generation = generation.wrapping_add(1);
        self.inner.changed.notify_all();
        true
    }

    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Blocks without polling until cancellation is requested.
    pub fn wait(&self) {
        let mut generation = self
            .inner
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        while !self.is_cancelled() {
            generation = self
                .inner
                .changed
                .wait(generation)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Waits up to `timeout`; true means cancellation was observed.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let generation = self
            .inner
            .generation
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = self
            .inner
            .changed
            .wait_timeout_while(generation, timeout, |_| !self.is_cancelled())
            .unwrap_or_else(|e| e.into_inner());
        self.is_cancelled()
    }
}

/// Stage 5 name for graph cancellation. It intentionally shares StopToken's
/// single atomic state rather than introducing a second cancellation tree.
pub type Cancellation = StopToken;
