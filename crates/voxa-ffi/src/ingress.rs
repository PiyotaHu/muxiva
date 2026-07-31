use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

use crate::frame::OwnedFrame;

pub struct ExternalIngress {
    capacity: usize,
    byte_capacity: usize,
    closed: AtomicBool,
    queue: Mutex<Queue>,
    accepted: AtomicU64,
    full: AtomicU64,
    closed_drops: AtomicU64,
}

struct Queue {
    frames: VecDeque<OwnedFrame>,
    bytes: usize,
}

impl ExternalIngress {
    pub fn new(capacity: usize, byte_capacity: usize) -> Self {
        Self {
            capacity,
            byte_capacity,
            closed: AtomicBool::new(false),
            queue: Mutex::new(Queue {
                frames: VecDeque::new(),
                bytes: 0,
            }),
            accepted: AtomicU64::new(0),
            full: AtomicU64::new(0),
            closed_drops: AtomicU64::new(0),
        }
    }

    pub fn try_submit(&self, frame: OwnedFrame) -> Result<(), SubmitError> {
        if self.closed.load(Ordering::Acquire) {
            self.closed_drops.fetch_add(1, Ordering::Relaxed);
            return Err(SubmitError::Closed);
        }
        let bytes = frame.copied_payload_len();
        let mut queue = self.queue.try_lock().map_err(|_| {
            self.full.fetch_add(1, Ordering::Relaxed);
            SubmitError::Full
        })?;
        if self.closed.load(Ordering::Acquire) {
            self.closed_drops.fetch_add(1, Ordering::Relaxed);
            return Err(SubmitError::Closed);
        }
        if queue.frames.len() >= self.capacity
            || queue
                .bytes
                .checked_add(bytes)
                .is_none_or(|total| total > self.byte_capacity)
        {
            self.full.fetch_add(1, Ordering::Relaxed);
            return Err(SubmitError::Full);
        }
        queue.bytes += bytes;
        queue.frames.push_back(frame);
        self.accepted.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    pub fn pop(&self) -> Option<OwnedFrame> {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        let frame = queue.frames.pop_front()?;
        queue.bytes -= frame.copied_payload_len();
        Some(frame)
    }

    pub fn stats(&self) -> (u64, u64, u64, usize, usize) {
        let queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        (
            self.accepted.load(Ordering::Relaxed),
            self.full.load(Ordering::Relaxed),
            self.closed_drops.load(Ordering::Relaxed),
            queue.frames.len(),
            queue.bytes,
        )
    }
}

pub enum SubmitError {
    Full,
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{OwnedFrame, OwnedHeader, OwnedPayload};

    fn text(bytes: &str) -> OwnedFrame {
        OwnedFrame {
            header: OwnedHeader {
                frame_type: 3,
                clock_kind: 2,
                timestamp_ns: 0,
                sequence_id: 1,
                frame_id: "ingress-frame".into(),
                clock_domain_id: "ingress.clock".into(),
                stream_id: "ingress-stream".into(),
                trace_id: "ingress-trace".into(),
            },
            payload: OwnedPayload::Text(bytes.into()),
        }
    }

    #[test]
    fn item_and_byte_limits_are_nonblocking_and_accounted() {
        let ingress = ExternalIngress::new(2, 5);
        assert!(ingress.try_submit(text("abc")).is_ok());
        assert!(matches!(
            ingress.try_submit(text("def")),
            Err(SubmitError::Full)
        ));
        assert_eq!(ingress.stats(), (1, 1, 0, 1, 3));
        assert_eq!(ingress.pop().unwrap().copied_payload_len(), 3);
        assert!(ingress.try_submit(text("def")).is_ok());
        ingress.close();
        assert!(matches!(
            ingress.try_submit(text("x")),
            Err(SubmitError::Closed)
        ));
        assert_eq!(ingress.stats(), (2, 1, 1, 1, 3));
    }
}
