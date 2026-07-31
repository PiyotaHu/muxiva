use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use voxa_types::SignalFrame;

use crate::{queue::QueueWake, DrainMode};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalQueuePushError {
    Full,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalQueueSnapshot {
    pub capacity: usize,
    pub queue_len: usize,
    pub enqueue_total: u64,
    pub dequeue_total: u64,
    pub full_total: u64,
}

struct State {
    queue: VecDeque<SignalFrame>,
    closed: bool,
    enqueue_total: u64,
    dequeue_total: u64,
    full_total: u64,
}

struct Inner {
    capacity: usize,
    state: Mutex<State>,
    target_wake: Arc<QueueWake>,
}

#[derive(Clone)]
pub(crate) struct SignalQueue(Arc<Inner>);

impl SignalQueue {
    pub(crate) fn new(capacity: usize, target_wake: Arc<QueueWake>) -> Self {
        debug_assert!(capacity > 0);
        Self(Arc::new(Inner {
            capacity,
            state: Mutex::new(State {
                queue: VecDeque::with_capacity(capacity),
                closed: false,
                enqueue_total: 0,
                dequeue_total: 0,
                full_total: 0,
            }),
            target_wake,
        }))
    }

    pub(crate) fn try_push(&self, signal: SignalFrame) -> Result<(), SignalQueuePushError> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return Err(SignalQueuePushError::Closed);
        }
        if state.queue.len() == self.0.capacity {
            state.full_total = state.full_total.saturating_add(1);
            return Err(SignalQueuePushError::Full);
        }
        state.queue.push_back(signal);
        state.enqueue_total = state.enqueue_total.saturating_add(1);
        drop(state);
        self.0.target_wake.notify();
        Ok(())
    }

    pub(crate) fn try_pop(&self) -> Option<SignalFrame> {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let signal = state.queue.pop_front();
        if signal.is_some() {
            state.dequeue_total = state.dequeue_total.saturating_add(1);
        }
        signal
    }

    pub(crate) fn close(&self, mode: DrainMode) {
        let mut state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        if mode == DrainMode::Discard {
            state.queue.clear();
        }
        drop(state);
        self.0.target_wake.notify();
    }

    pub(crate) fn is_closed_and_empty(&self) -> bool {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.closed && state.queue.is_empty()
    }

    pub(crate) fn snapshot(&self) -> SignalQueueSnapshot {
        let state = self
            .0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        SignalQueueSnapshot {
            capacity: self.0.capacity,
            queue_len: state.queue.len(),
            enqueue_total: state.enqueue_total,
            dequeue_total: state.dequeue_total,
            full_total: state.full_total,
        }
    }
}
