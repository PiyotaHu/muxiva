use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use muxiva_core::FlowClock;

/// A monotonic clock advanced only by its test controller.
#[derive(Debug, Default)]
pub struct ManualClock {
    nanos: AtomicU64,
}

impl ManualClock {
    pub const fn new() -> Self {
        Self {
            nanos: AtomicU64::new(0),
        }
    }

    pub fn advance(&self, duration: Duration) -> Duration {
        let delta = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        let previous = self.nanos.fetch_add(delta, Ordering::AcqRel);
        Duration::from_nanos(previous.saturating_add(delta))
    }

    pub fn set(&self, value: Duration) {
        let nanos = u64::try_from(value.as_nanos()).unwrap_or(u64::MAX);
        self.nanos.store(nanos, Ordering::Release);
    }
}

impl FlowClock for ManualClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::Acquire))
    }
}
