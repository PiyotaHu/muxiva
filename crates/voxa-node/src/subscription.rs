use std::collections::BTreeMap;

use napi::{
    threadsafe_function::{
        ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
    },
    Error, JsFunction, Result, Status,
};

struct Subscriber {
    topic: String,
    callback: ThreadsafeFunction<String, ErrorStrategy::Fatal>,
}

#[derive(Default)]
pub(crate) struct SubscriptionSet {
    next: u32,
    closed: bool,
    subscribers: BTreeMap<u32, Subscriber>,
}

impl SubscriptionSet {
    pub fn subscribe(&mut self, topic: String, callback: JsFunction, capacity: u32) -> Result<u32> {
        if self.closed {
            return Err(Error::new(Status::Closing, "EventBus is closed"));
        }
        if !(1..=65_536).contains(&capacity) || !topic.contains('.') {
            return Err(Error::new(Status::InvalidArg, "invalid topic or capacity"));
        }
        let callback = callback.create_threadsafe_function(
            capacity as usize,
            |ctx: ThreadSafeCallContext<String>| Ok(vec![ctx.value]),
        )?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::new(Status::GenericFailure, "subscription id exhausted"))?;
        self.subscribers
            .insert(self.next, Subscriber { topic, callback });
        Ok(self.next)
    }
    pub fn publish(&self, topic: &str, payload: String) -> Result<u32> {
        if self.closed {
            return Err(Error::new(Status::Closing, "EventBus is closed"));
        }
        let mut accepted = 0;
        for subscriber in self
            .subscribers
            .values()
            .filter(|entry| entry.topic == topic)
        {
            if subscriber
                .callback
                .call(payload.clone(), ThreadsafeFunctionCallMode::NonBlocking)
                == Status::Ok
            {
                accepted += 1;
            }
        }
        Ok(accepted)
    }
    pub fn unsubscribe(&mut self, id: u32) -> bool {
        self.subscribers
            .remove(&id)
            .map(|subscriber| {
                let _ = subscriber.callback.abort();
            })
            .is_some()
    }
    pub fn close(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.closed = true;
        for (_, subscriber) in std::mem::take(&mut self.subscribers) {
            let _ = subscriber.callback.abort();
        }
        true
    }
}
