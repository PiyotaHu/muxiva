use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    num::NonZeroUsize,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use muxiva_types::{EventFrame, NamespacedName, Result as MuxivaResult};

/// Opaque identity returned by [`NotificationBus::subscribe`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Subscription(u64);

impl Subscription {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubscriberSnapshot {
    pub delivered: u64,
    pub dropped_full: u64,
    pub handler_errors: u64,
    pub handler_panics: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotificationBusError {
    Stopped,
    Spawn { thread_name: Box<str> },
}

impl fmt::Display for NotificationBusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped => formatter.write_str("notification bus is stopped"),
            Self::Spawn { thread_name } => {
                write!(
                    formatter,
                    "failed to start NotificationBus subscriber `{thread_name}`"
                )
            }
        }
    }
}

impl Error for NotificationBusError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PublishReport {
    pub matched: usize,
    pub enqueued: usize,
    pub dropped_full: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NotificationBusStopReport {
    pub stopped_first: bool,
    pub worker_total: usize,
    pub unfinished: Box<[Subscription]>,
}

#[derive(Default)]
struct MutableMetrics {
    delivered: u64,
    dropped_full: u64,
    handler_errors: u64,
    handler_panics: u64,
}

impl MutableMetrics {
    fn snapshot(&self) -> SubscriberSnapshot {
        SubscriberSnapshot {
            delivered: self.delivered,
            dropped_full: self.dropped_full,
            handler_errors: self.handler_errors,
            handler_panics: self.handler_panics,
        }
    }
}

struct SubscriberWorker {
    id: Subscription,
    topic: NamespacedName,
    sender: Option<mpsc::SyncSender<EventFrame>>,
    done: mpsc::Receiver<()>,
    handle: Option<thread::JoinHandle<()>>,
    metrics: Arc<Mutex<MutableMetrics>>,
}

#[derive(Default)]
struct BusState {
    stopped: bool,
    active: BTreeMap<Subscription, SubscriberWorker>,
    retired: Vec<SubscriberWorker>,
    observations: BTreeMap<Subscription, Arc<Mutex<MutableMetrics>>>,
}

struct NotificationBusInner {
    next_id: AtomicU64,
    state: Mutex<BusState>,
}

/// Process-local, low-frequency notification fanout with one bounded worker per subscriber.
///
/// Notifications currently use `EventFrame` as their typed envelope, but they do
/// not travel over Graph ports or Edge queues.
#[derive(Clone)]
pub struct NotificationBus {
    inner: Arc<NotificationBusInner>,
    capacity: NonZeroUsize,
}

impl NotificationBus {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Arc::new(NotificationBusInner {
                next_id: AtomicU64::new(1),
                state: Mutex::new(BusState::default()),
            }),
            capacity,
        }
    }

    pub fn subscribe<F>(
        &self,
        topic: NamespacedName,
        handler: F,
    ) -> Result<Subscription, NotificationBusError>
    where
        F: Fn(EventFrame) -> MuxivaResult<()> + Send + 'static,
    {
        let id = Subscription(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let (sender, receiver) = mpsc::sync_channel(self.capacity.get());
        let (done_tx, done) = mpsc::channel();
        let metrics = Arc::new(Mutex::new(MutableMetrics::default()));
        let worker_metrics = metrics.clone();
        let thread_name = format!("muxiva-notification-subscriber-{}", id.get());
        let handle = thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || {
                for event in receiver {
                    let outcome = catch_unwind(AssertUnwindSafe(|| handler(event)));
                    let mut metrics = worker_metrics.lock().unwrap_or_else(|e| e.into_inner());
                    match outcome {
                        Ok(Ok(())) => metrics.delivered = metrics.delivered.saturating_add(1),
                        Ok(Err(_)) => {
                            metrics.handler_errors = metrics.handler_errors.saturating_add(1)
                        }
                        Err(_) => metrics.handler_panics = metrics.handler_panics.saturating_add(1),
                    }
                }
                let _ = done_tx.send(());
            })
            .map_err(|_| NotificationBusError::Spawn {
                thread_name: thread_name.into(),
            })?;

        let worker = SubscriberWorker {
            id,
            topic,
            sender: Some(sender),
            done,
            handle: Some(handle),
            metrics: metrics.clone(),
        };
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            drop(state);
            drop(worker);
            return Err(NotificationBusError::Stopped);
        }
        state.observations.insert(id, metrics);
        state.active.insert(id, worker);
        Ok(id)
    }

    /// Enqueues without waiting for any subscriber or handler.
    pub fn publish(&self, event: EventFrame) -> Result<PublishReport, NotificationBusError> {
        let topic = event.data().topic();
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return Err(NotificationBusError::Stopped);
        }
        let mut report = PublishReport::default();
        for worker in state
            .active
            .values_mut()
            .filter(|worker| &worker.topic == topic)
        {
            report.matched += 1;
            let Some(sender) = worker.sender.as_ref() else {
                continue;
            };
            match sender.try_send(event.clone()) {
                Ok(()) => report.enqueued += 1,
                Err(mpsc::TrySendError::Full(_)) => {
                    report.dropped_full += 1;
                    let mut metrics = worker.metrics.lock().unwrap_or_else(|e| e.into_inner());
                    metrics.dropped_full = metrics.dropped_full.saturating_add(1);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {}
            }
        }
        Ok(report)
    }

    /// Removes a subscription immediately. A running handler is reaped by `stop`.
    pub fn unsubscribe(&self, subscription: Subscription) -> bool {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(mut worker) = state.active.remove(&subscription) else {
            return false;
        };
        worker.sender.take();
        state.retired.push(worker);
        true
    }

    pub fn subscriber_snapshot(&self, subscription: Subscription) -> Option<SubscriberSnapshot> {
        let metrics = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observations
            .get(&subscription)
            .cloned()?;
        let snapshot = metrics.lock().unwrap_or_else(|e| e.into_inner()).snapshot();
        Some(snapshot)
    }

    /// Rejects new subscriptions/publications and disconnects subscriber mailboxes without waiting.
    pub fn request_stop(&self) -> bool {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        let first = !state.stopped;
        state.stopped = true;
        let active = std::mem::take(&mut state.active);
        for (_, mut worker) in active {
            worker.sender.take();
            state.retired.push(worker);
        }
        first
    }

    /// Stops admission, disconnects all mailboxes, and reaps workers until the deadline.
    pub fn stop(&self, timeout: Duration) -> NotificationBusStopReport {
        let stopped_first = self.request_stop();
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut workers = std::mem::take(&mut state.retired);
        for worker in &mut workers {
            worker.sender.take();
        }
        drop(state);

        let deadline = Instant::now() + timeout;
        let worker_total = workers.len();
        let mut unfinished = Vec::new();
        for mut worker in workers {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if worker.done.recv_timeout(remaining).is_ok() {
                if let Some(handle) = worker.handle.take() {
                    let _ = handle.join();
                }
            } else {
                unfinished.push(worker.id);
            }
        }
        NotificationBusStopReport {
            stopped_first,
            worker_total,
            unfinished: unfinished.into_boxed_slice(),
        }
    }
}

impl Default for NotificationBus {
    fn default() -> Self {
        Self::new(NonZeroUsize::new(64).expect("non-zero constant"))
    }
}
