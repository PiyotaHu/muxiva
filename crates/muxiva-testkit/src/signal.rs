use muxiva_types::SignalFrame;
use std::sync::{Arc, Mutex};
#[derive(Clone, Default)]
pub struct SignalProbe(Arc<Mutex<Vec<SignalFrame>>>);
impl SignalProbe {
    pub fn record(&self, signal: SignalFrame) {
        self.0.lock().unwrap().push(signal)
    }
    pub fn snapshot(&self) -> Vec<SignalFrame> {
        self.0.lock().unwrap().clone()
    }
}
