use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use napi::{Error, Result, Status};
use napi_derive::napi;

use crate::subscription::SubscriptionSet;

fn closed() -> Error {
    Error::new(Status::Closing, "Voxa handle is closed")
}

/// An explicit lifetime boundary. Graph execution is deliberately not exposed
/// until a general Core graph/session API exists.
#[napi]
pub struct Runtime {
    closed: AtomicBool,
    next_session: AtomicU64,
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[napi]
impl Runtime {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            closed: AtomicBool::new(false),
            next_session: AtomicU64::new(1),
        }
    }
    #[napi]
    pub fn create_session(&self) -> Result<Session> {
        if self.closed.load(Ordering::Acquire) {
            return Err(closed());
        }
        Ok(Session {
            id: self.next_session.fetch_add(1, Ordering::Relaxed),
            closed: AtomicBool::new(false),
        })
    }
    #[napi(getter)]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    #[napi]
    pub fn close(&self) -> bool {
        !self.closed.swap(true, Ordering::AcqRel)
    }
}

#[napi]
pub struct Session {
    id: u64,
    closed: AtomicBool,
}

#[napi]
impl Session {
    #[napi(getter)]
    pub fn id(&self) -> i64 {
        self.id as i64
    }
    #[napi(getter)]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    #[napi]
    pub fn close(&self) -> bool {
        !self.closed.swap(true, Ordering::AcqRel)
    }
}

#[napi]
pub struct EventBus {
    subscriptions: SubscriptionSet,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[napi]
impl EventBus {
    #[napi(constructor)]
    pub fn new() -> Self {
        Self {
            subscriptions: SubscriptionSet::default(),
        }
    }
    #[napi]
    pub fn subscribe(
        &mut self,
        topic: String,
        callback: napi::JsFunction,
        capacity: Option<u32>,
    ) -> Result<u32> {
        self.subscriptions
            .subscribe(topic, callback, capacity.unwrap_or(16))
    }
    #[napi]
    pub fn publish(&self, topic: String, payload_json: String) -> Result<u32> {
        self.subscriptions.publish(&topic, payload_json)
    }
    #[napi]
    pub fn unsubscribe(&mut self, id: u32) -> bool {
        self.subscriptions.unsubscribe(id)
    }
    #[napi]
    pub fn close(&mut self) -> bool {
        self.subscriptions.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_is_idempotent() {
        let runtime = Runtime::new();
        assert!(runtime.close());
        assert!(!runtime.close());
        assert!(runtime.create_session().is_err());
    }
}
