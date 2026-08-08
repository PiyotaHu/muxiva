use muxiva_core::{AdapterRequest, AdapterResponse, ManagedStreamAdapter, ServiceError};
use std::sync::{Arc, Mutex};
#[derive(Clone, Debug)]
pub enum ManagedOutcome {
    Echo,
    Retryable(Box<str>),
    Failed(Box<str>),
}
pub struct ScriptedManagedStreamAdapter {
    outcomes: Mutex<Vec<ManagedOutcome>>,
    requests: Arc<Mutex<Vec<(u64, usize)>>>,
}
impl ScriptedManagedStreamAdapter {
    pub fn new(outcomes: Vec<ManagedOutcome>, requests: Arc<Mutex<Vec<(u64, usize)>>>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes),
            requests,
        }
    }
}
impl ManagedStreamAdapter for ScriptedManagedStreamAdapter {
    fn send(&self, request: AdapterRequest) -> AdapterResponse {
        self.requests
            .lock()
            .unwrap()
            .push((request.request_id.get(), request.attempt));
        let outcome = {
            let mut outcomes = self.outcomes.lock().unwrap();
            if outcomes.is_empty() {
                ManagedOutcome::Echo
            } else {
                outcomes.remove(0)
            }
        };
        match outcome {
            ManagedOutcome::Echo => AdapterResponse::Frames(vec![request.input]),
            ManagedOutcome::Retryable(m) => {
                AdapterResponse::Retryable(ServiceError::new("test_retry", m))
            }
            ManagedOutcome::Failed(m) => {
                AdapterResponse::Failed(ServiceError::new("test_failed", m))
            }
        }
    }
}
