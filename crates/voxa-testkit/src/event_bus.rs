use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use voxa_core::{EventBus, EventBusError, Subscription};
use voxa_types::{EventFrame, NamespacedName};
pub struct EventBusProbe {
    bus: EventBus,
    events: Arc<Mutex<Vec<EventFrame>>>,
}
impl EventBusProbe {
    pub fn new(capacity: usize) -> Self {
        Self {
            bus: EventBus::new(NonZeroUsize::new(capacity).unwrap()),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub fn subscribe(&self, topic: NamespacedName) -> Result<Subscription, EventBusError> {
        let events = self.events.clone();
        self.bus.subscribe(topic, move |event| {
            events.lock().unwrap().push(event);
            Ok(())
        })
    }
    pub fn bus(&self) -> &EventBus {
        &self.bus
    }
    pub fn snapshot(&self) -> Vec<EventFrame> {
        self.events.lock().unwrap().clone()
    }
}
