use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use muxiva_types::{EdgeId, Frame};

use crate::{EdgeMetricsSnapshot, QueueOverflowPolicy, QueuePolicy};

/// Whether queued frames remain available after an Edge is closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DrainMode {
    /// Reject new frames but let consumers receive frames already queued.
    Drain,
    /// Reject new frames and immediately discard all queued frames.
    Discard,
}

/// Stable, observable reason for a queue-level frame drop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueDropReason {
    QueueFullDropOldest,
    QueueFullDropNewest,
    ShutdownDiscard,
}

impl QueueDropReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueueFullDropOldest => "queue full: dropped oldest frame",
            Self::QueueFullDropNewest => "queue full: dropped newest frame",
            Self::ShutdownDiscard => "queue closed: discarded buffered frame",
        }
    }
}

/// Result of submitting one concrete Frame to an Edge queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnqueueOutcome {
    Enqueued,
    Dropped(QueueDropReason),
    EnqueuedAfterDroppingOldest,
}

/// Why a producer could not submit a frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueuePushError {
    Closed,
    OverflowAbort,
}

impl fmt::Display for QueuePushError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("Edge queue is closed"),
            Self::OverflowAbort => formatter.write_str("Edge queue overflow policy aborted"),
        }
    }
}

impl std::error::Error for QueuePushError {}

/// A closed and drained queue has no further frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueClosed;

impl fmt::Display for QueueClosed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Edge queue is closed and empty")
    }
}

impl std::error::Error for QueueClosed {}

#[derive(Default)]
pub(crate) struct QueueWake {
    generation: Mutex<u64>,
    changed: Condvar,
}

impl QueueWake {
    pub(crate) fn generation(&self) -> u64 {
        *self.generation.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn notify(&self) {
        let mut generation = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        *generation = generation.wrapping_add(1);
        self.changed.notify_all();
    }

    pub(crate) fn wait_for_change(&self, observed: u64) {
        let mut generation = self.generation.lock().unwrap_or_else(|e| e.into_inner());
        while *generation == observed {
            generation = self
                .changed
                .wait(generation)
                .unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// Bounded, thread-safe Edge queue. Its data elements are always Frame; queue
/// timestamps are retained in a parallel metadata deque.
#[derive(Clone)]
pub struct EdgeQueue {
    inner: Arc<QueueInner>,
}

struct QueueInner {
    edge_id: EdgeId,
    capacity: usize,
    overflow: QueueOverflowPolicy,
    state: Mutex<QueueState>,
    not_empty: Condvar,
    not_full: Condvar,
    target_wake: Option<Arc<QueueWake>>,
}

struct QueueState {
    frames: VecDeque<Frame>,
    enqueued_at: VecDeque<Instant>,
    closed: bool,
    high_watermark: usize,
    enqueue_total: u64,
    dequeue_total: u64,
    drop_total: u64,
    signal_total: u64,
    full_total: u64,
    blocked_duration_ns: u64,
    payload_bytes_total: u64,
    audio_duration_ns_total: u64,
    latest_error_reason: Option<Box<str>>,
}

impl EdgeQueue {
    /// Allocates one bounded queue from the graph's declarative queue policy.
    pub fn new(edge_id: EdgeId, policy: QueuePolicy) -> Self {
        Self::with_target_wake(edge_id, policy, None)
    }

    pub(crate) fn with_target_wake(
        edge_id: EdgeId,
        policy: QueuePolicy,
        target_wake: Option<Arc<QueueWake>>,
    ) -> Self {
        Self {
            inner: Arc::new(QueueInner {
                edge_id,
                capacity: policy.capacity().get(),
                overflow: policy.overflow(),
                state: Mutex::new(QueueState {
                    frames: VecDeque::new(),
                    enqueued_at: VecDeque::new(),
                    closed: false,
                    high_watermark: 0,
                    enqueue_total: 0,
                    dequeue_total: 0,
                    drop_total: 0,
                    signal_total: 0,
                    full_total: 0,
                    blocked_duration_ns: 0,
                    payload_bytes_total: 0,
                    audio_duration_ns_total: 0,
                    latest_error_reason: None,
                }),
                not_empty: Condvar::new(),
                not_full: Condvar::new(),
                target_wake,
            }),
        }
    }

    /// Submits a frame, blocking only for the declarative Block policy.
    pub fn push(&self, frame: Frame) -> Result<EnqueueOutcome, QueuePushError> {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Err(QueuePushError::Closed);
        }
        let mut blocked_at = None;
        while state.frames.len() == self.inner.capacity {
            state.full_total = state.full_total.saturating_add(1);
            match self.inner.overflow {
                QueueOverflowPolicy::Block => {
                    let started = Instant::now();
                    blocked_at.get_or_insert(started);
                    state = self
                        .inner
                        .not_full
                        .wait(state)
                        .unwrap_or_else(|e| e.into_inner());
                    if state.closed {
                        add_blocked_duration(&mut state, blocked_at);
                        return Err(QueuePushError::Closed);
                    }
                }
                QueueOverflowPolicy::DropOldest => {
                    state.frames.pop_front();
                    state.enqueued_at.pop_front();
                    state.drop_total = state.drop_total.saturating_add(1);
                    set_reason(&mut state, QueueDropReason::QueueFullDropOldest.as_str());
                    enqueue(&mut state, frame);
                    drop(state);
                    self.notify_available();
                    return Ok(EnqueueOutcome::EnqueuedAfterDroppingOldest);
                }
                QueueOverflowPolicy::DropNewest => {
                    state.drop_total = state.drop_total.saturating_add(1);
                    set_reason(&mut state, QueueDropReason::QueueFullDropNewest.as_str());
                    return Ok(EnqueueOutcome::Dropped(
                        QueueDropReason::QueueFullDropNewest,
                    ));
                }
                QueueOverflowPolicy::Abort => {
                    set_reason(&mut state, "queue full: overflow policy abort");
                    return Err(QueuePushError::OverflowAbort);
                }
            }
        }
        add_blocked_duration(&mut state, blocked_at);
        enqueue(&mut state, frame);
        drop(state);
        self.notify_available();
        Ok(EnqueueOutcome::Enqueued)
    }

    /// Receives the next frame, blocking without polling until data or close.
    pub fn pop(&self) -> Result<Frame, QueueClosed> {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(frame) = dequeue(&mut state) {
                self.inner.not_full.notify_one();
                return Ok(frame);
            }
            if state.closed {
                return Err(QueueClosed);
            }
            state = self
                .inner
                .not_empty
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Non-blocking receive used by a node worker with several incoming Edges.
    pub fn try_pop(&self) -> Result<Option<Frame>, QueueClosed> {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(frame) = dequeue(&mut state) {
            self.inner.not_full.notify_one();
            return Ok(Some(frame));
        }
        if state.closed {
            Err(QueueClosed)
        } else {
            Ok(None)
        }
    }

    /// Closes the queue and wakes all blocked producers and consumers.
    pub fn close(&self, mode: DrainMode) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        if !state.closed {
            state.closed = true;
        }
        // Close is monotonic: a later Discard escalates an earlier Drain, but
        // a later Drain can never make discarded data available again.
        if mode == DrainMode::Discard {
            let discarded = state.frames.len() as u64;
            state.frames.clear();
            state.enqueued_at.clear();
            if discarded != 0 {
                state.drop_total = state.drop_total.saturating_add(discarded);
                set_reason(&mut state, QueueDropReason::ShutdownDiscard.as_str());
            }
        }
        drop(state);
        self.inner.not_empty.notify_all();
        self.inner.not_full.notify_all();
        if let Some(wake) = &self.inner.target_wake {
            wake.notify();
        }
    }

    /// Returns a coherent EdgeId-keyed queue metrics snapshot.
    pub fn snapshot(&self) -> EdgeMetricsSnapshot {
        let state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        EdgeMetricsSnapshot::from_runtime(
            self.inner.edge_id.clone(),
            self.inner.capacity,
            state.frames.len(),
            state.high_watermark,
            state.enqueue_total,
            state.dequeue_total,
            state.drop_total,
            state.signal_total,
            state.full_total,
            state.blocked_duration_ns,
            state.enqueued_at.front().map(|at| nanos(at.elapsed())),
            state.payload_bytes_total,
            state.audio_duration_ns_total,
            state.latest_error_reason.clone(),
        )
    }

    pub(crate) fn is_closed_and_empty(&self) -> bool {
        let state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed && state.frames.is_empty()
    }

    pub(crate) fn record_drop(&self, reason: &str) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.drop_total = state.drop_total.saturating_add(1);
        set_reason(&mut state, reason);
    }

    pub(crate) fn record_signal(&self) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        state.signal_total = state.signal_total.saturating_add(1);
    }

    pub(crate) fn record_error(&self, reason: &str) {
        let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
        set_reason(&mut state, reason);
    }

    fn notify_available(&self) {
        self.inner.not_empty.notify_one();
        if let Some(wake) = &self.inner.target_wake {
            wake.notify();
        }
    }
}

fn enqueue(state: &mut QueueState, frame: Frame) {
    let (payload_bytes, audio_duration_ns) = frame_measurement(&frame);
    state.frames.push_back(frame);
    state.enqueued_at.push_back(Instant::now());
    state.enqueue_total = state.enqueue_total.saturating_add(1);
    state.payload_bytes_total = state.payload_bytes_total.saturating_add(payload_bytes);
    state.audio_duration_ns_total = state
        .audio_duration_ns_total
        .saturating_add(audio_duration_ns);
    state.high_watermark = state.high_watermark.max(state.frames.len());
}

fn frame_measurement(frame: &Frame) -> (u64, u64) {
    if let Some(audio) = frame.as_audio() {
        let data = audio.data();
        let bytes = u64::try_from(data.buffer().as_slice().len()).unwrap_or(u64::MAX);
        let duration = data
            .samples_per_channel()
            .saturating_mul(1_000_000_000)
            .checked_div(u64::from(data.sample_rate_hz()))
            .unwrap_or(0);
        return (bytes, duration);
    }
    if let Some(bytes) = frame.as_byte() {
        return (
            u64::try_from(bytes.data().buffer().as_slice().len()).unwrap_or(u64::MAX),
            0,
        );
    }
    if let Some(text) = frame.as_text() {
        return (
            u64::try_from(text.data().as_str().len()).unwrap_or(u64::MAX),
            0,
        );
    }
    (0, 0)
}

fn dequeue(state: &mut QueueState) -> Option<Frame> {
    let frame = state.frames.pop_front()?;
    state.enqueued_at.pop_front();
    state.dequeue_total = state.dequeue_total.saturating_add(1);
    Some(frame)
}

fn add_blocked_duration(state: &mut QueueState, blocked_at: Option<Instant>) {
    if let Some(started) = blocked_at {
        state.blocked_duration_ns = state
            .blocked_duration_ns
            .saturating_add(nanos(started.elapsed()));
    }
}

fn nanos(duration: std::time::Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn set_reason(state: &mut QueueState, reason: &str) {
    const MAX_REASON_BYTES: usize = 256;
    let mut value = String::new();
    for character in reason.chars() {
        let character = if character.is_ascii_control() {
            ' '
        } else {
            character
        };
        if value.len() + character.len_utf8() > MAX_REASON_BYTES {
            break;
        }
        value.push(character);
    }
    state.latest_error_reason = Some(value.into_boxed_str());
}
