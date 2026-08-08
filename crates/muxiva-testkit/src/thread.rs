use std::{
    collections::VecDeque,
    sync::Mutex,
    thread::{self, ThreadId},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadEvent {
    pub role: Box<str>,
    pub event: Box<str>,
    pub thread_id: ThreadId,
}

/// A bounded recorder for asserting execution-domain separation.
#[derive(Debug)]
pub struct ThreadProbe {
    capacity: usize,
    events: Mutex<VecDeque<ThreadEvent>>,
}

impl ThreadProbe {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "thread probe capacity must be non-zero");
        Self {
            capacity,
            events: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }

    pub fn record(&self, role: impl Into<Box<str>>, event: impl Into<Box<str>>) {
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if events.len() == self.capacity {
            events.pop_front();
        }
        events.push_back(ThreadEvent {
            role: role.into(),
            event: event.into(),
            thread_id: thread::current().id(),
        });
    }

    pub fn snapshot(&self) -> Vec<ThreadEvent> {
        self.events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}
