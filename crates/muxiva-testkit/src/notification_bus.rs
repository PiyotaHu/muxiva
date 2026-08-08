use muxiva_core::{NotificationBus, NotificationBusError, Subscription};
use muxiva_types::{EventFrame, NamespacedName};
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
pub struct NotificationBusProbe {
    bus: NotificationBus,
    events: Arc<Mutex<Vec<EventFrame>>>,
}
impl NotificationBusProbe {
    pub fn new(capacity: usize) -> Self {
        Self {
            bus: NotificationBus::new(NonZeroUsize::new(capacity).unwrap()),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub fn subscribe(&self, topic: NamespacedName) -> Result<Subscription, NotificationBusError> {
        let events = self.events.clone();
        self.bus.subscribe(topic, move |event| {
            events.lock().unwrap().push(event);
            Ok(())
        })
    }
    pub fn bus(&self) -> &NotificationBus {
        &self.bus
    }
    pub fn snapshot(&self) -> Vec<EventFrame> {
        self.events.lock().unwrap().clone()
    }
}
