use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use napi::{
    bindgen_prelude::Function,
    threadsafe_function::{ThreadsafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode},
    Error, Result, Status,
};

struct Subscriber {
    topic: String,
    callback: ThreadsafeFunction<SubscriptionCall, (), String, Status, false, false, 65_536>,
    pending: Arc<AtomicU32>,
    capacity: u32,
}

struct SubscriptionCall {
    payload: String,
    pending: Arc<AtomicU32>,
}

#[derive(Default)]
pub(crate) struct SubscriptionSet {
    next: u32,
    closed: bool,
    subscribers: BTreeMap<u32, Subscriber>,
}

impl SubscriptionSet {
    pub fn subscribe(
        &mut self,
        topic: String,
        callback: Function<'_, String, ()>,
        capacity: u32,
    ) -> Result<u32> {
        if self.closed {
            return Err(Error::new(Status::Closing, "NotificationBus is closed"));
        }
        if !(1..=65_536).contains(&capacity) || !topic.contains('.') {
            return Err(Error::new(Status::InvalidArg, "invalid topic or capacity"));
        }
        let callback = callback
            .build_threadsafe_function::<SubscriptionCall>()
            .max_queue_size::<65_536>()
            .build_callback(|ctx: ThreadsafeCallContext<SubscriptionCall>| {
                ctx.value.pending.fetch_sub(1, Ordering::AcqRel);
                Ok(ctx.value.payload)
            })?;
        let pending = Arc::new(AtomicU32::new(0));
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| Error::new(Status::GenericFailure, "subscription id exhausted"))?;
        self.subscribers.insert(
            self.next,
            Subscriber {
                topic,
                callback,
                pending,
                capacity,
            },
        );
        Ok(self.next)
    }
    pub fn publish(&self, topic: &str, payload: String) -> Result<u32> {
        if self.closed {
            return Err(Error::new(Status::Closing, "NotificationBus is closed"));
        }
        let mut accepted = 0;
        for subscriber in self
            .subscribers
            .values()
            .filter(|entry| entry.topic == topic)
        {
            let reserved = subscriber
                .pending
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                    (pending < subscriber.capacity).then_some(pending + 1)
                })
                .is_ok();
            if !reserved {
                continue;
            }
            let status = subscriber.callback.call(
                SubscriptionCall {
                    payload: payload.clone(),
                    pending: Arc::clone(&subscriber.pending),
                },
                ThreadsafeFunctionCallMode::NonBlocking,
            );
            if status == Status::Ok {
                accepted += 1;
            } else {
                subscriber.pending.fetch_sub(1, Ordering::AcqRel);
            }
        }
        Ok(accepted)
    }
    pub fn unsubscribe(&mut self, id: u32) -> bool {
        self.subscribers.remove(&id).map(drop).is_some()
    }
    pub fn close(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.closed = true;
        drop(std::mem::take(&mut self.subscribers));
        true
    }
}
