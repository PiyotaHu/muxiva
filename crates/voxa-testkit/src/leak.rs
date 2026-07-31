use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeakSnapshot {
    pub created: u64,
    pub released: u64,
    pub destroyed: u64,
}

impl LeakSnapshot {
    pub const fn outstanding(self) -> u64 {
        self.created.saturating_sub(self.destroyed)
    }
}

/// Test-owned lifecycle accounting. Production handles are never wrapped.
#[derive(Debug, Default)]
pub struct LeakProbe {
    created: AtomicU64,
    released: AtomicU64,
    destroyed: AtomicU64,
}

impl LeakProbe {
    pub fn record_create(&self) {
        self.created.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_release(&self) {
        self.released.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_destroy(&self) {
        self.destroyed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> LeakSnapshot {
        LeakSnapshot {
            created: self.created.load(Ordering::Acquire),
            released: self.released.load(Ordering::Acquire),
            destroyed: self.destroyed.load(Ordering::Acquire),
        }
    }
}
