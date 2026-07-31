//! Isolated, bounded execution for long-lived asynchronous service sessions.
//!
//! A managed stream is deliberately independent from graph workers. Submission
//! and result polling are non-blocking, while service adapters run on dedicated
//! threads behind a per-session in-flight window.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    error::Error,
    fmt,
    num::NonZeroUsize,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc, Condvar, Mutex, Weak,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use voxa_types::{Frame, SessionId};

use crate::{AdmissionLease, DeliveryOrdering};

/// Identifies one request within a managed service session.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(u64);

impl RequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A bounded service error safe to retain in runtime diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceError {
    code: Box<str>,
    message: Box<str>,
}

impl ServiceError {
    pub fn new(code: impl Into<Box<str>>, message: impl Into<Box<str>>) -> Self {
        Self {
            code: bounded_text(code.into(), 64),
            message: bounded_text(message.into(), 512),
        }
    }

    pub fn timeout() -> Self {
        Self::new(
            "managed_stream_timeout",
            "managed stream request deadline elapsed",
        )
    }

    pub fn worker_start() -> Self {
        Self::new(
            "managed_stream_worker_start",
            "managed stream could not start an isolated request worker",
        )
    }

    pub fn adapter_panic() -> Self {
        Self::new(
            "managed_stream_adapter_panic",
            "managed stream adapter panicked while handling a request",
        )
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for ServiceError {}

fn bounded_text(value: Box<str>, max_bytes: usize) -> Box<str> {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].into()
}

/// A request submitted by a graph-owned adapter.
pub struct AsyncRequest {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub input: Frame,
    pub deadline: Instant,
    pub attempt_limit: NonZeroUsize,
    pub admission: AdmissionLease,
}

/// The immediate outcome of a non-blocking submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitOutcome {
    Accepted,
    MailboxFull,
    Stopping,
    Cancelled,
}

/// A terminal service result. Retryable means the configured attempt limit was
/// reached while the adapter still reported a reconnectable failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamResult {
    Frames(Vec<Frame>),
    Retryable(ServiceError),
    Failed(ServiceError),
}

/// One result routed through the bounded result mailbox.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamCompletion {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub result: StreamResult,
}

/// The owned request view passed to a service adapter.
#[derive(Clone)]
pub struct AdapterRequest {
    pub request_id: RequestId,
    pub session_id: SessionId,
    pub input: Frame,
    pub attempt: usize,
}

/// Result of one service send/response exchange.
pub enum AdapterResponse {
    Frames(Vec<Frame>),
    Retryable(ServiceError),
    Failed(ServiceError),
}

/// Protocol adapter executed only on isolated managed-stream workers.
pub trait ManagedStreamAdapter: Send + Sync + 'static {
    /// Establishes or re-establishes the session transport. Connect errors are
    /// retryable within the request's attempt and deadline boundary.
    fn connect(&self, _session_id: &SessionId, _reconnecting: bool) -> Result<(), ServiceError> {
        Ok(())
    }

    /// Sends one request and parses its response into concrete Frames.
    fn send(&self, request: AdapterRequest) -> AdapterResponse;
}

impl<F> ManagedStreamAdapter for F
where
    F: Fn(AdapterRequest) -> AdapterResponse + Send + Sync + 'static,
{
    fn send(&self, request: AdapterRequest) -> AdapterResponse {
        self(request)
    }
}

/// Bounded resources owned by one service session.
#[derive(Clone, Debug)]
pub struct ManagedStreamOptions {
    pub input_capacity: NonZeroUsize,
    pub result_capacity: NonZeroUsize,
    pub max_in_flight: NonZeroUsize,
    pub ordering: DeliveryOrdering,
    pub reconnect_delay: Duration,
    pub thread_name: Box<str>,
}

impl Default for ManagedStreamOptions {
    fn default() -> Self {
        Self {
            input_capacity: NonZeroUsize::new(8).expect("non-zero constant"),
            result_capacity: NonZeroUsize::new(8).expect("non-zero constant"),
            max_in_flight: NonZeroUsize::new(1).expect("non-zero constant"),
            ordering: DeliveryOrdering::Strict,
            reconnect_delay: Duration::ZERO,
            thread_name: "voxa-managed-stream".into(),
        }
    }
}

#[derive(Debug)]
pub struct ManagedStreamBuildError(std::io::Error);

impl fmt::Display for ManagedStreamBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "failed to start managed stream executor: {}",
            self.0
        )
    }
}

impl Error for ManagedStreamBuildError {}

/// Observable lifecycle of a managed session executor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedStreamState {
    Running,
    Stopping,
    Stopped,
}

/// Public counters for capacity, isolation, and lifecycle diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedStreamMetricsSnapshot {
    pub state: ManagedStreamState,
    pub submitted: u64,
    pub accepted: u64,
    pub mailbox_full: u64,
    pub started: u64,
    pub attempts: u64,
    pub retries: u64,
    pub reconnects: u64,
    pub completed: u64,
    pub succeeded: u64,
    pub retry_exhausted: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub cancelled: u64,
    pub late_results_discarded: u64,
    pub result_backpressure: u64,
    pub results_delivered: u64,
    pub queued_inputs: usize,
    pub active_requests: usize,
    pub ordered_pending: usize,
    pub queued_results: usize,
    pub peak_active_requests: usize,
}

/// Bounded stop observation. A false `executor_finished` means only that the
/// supplied diagnostic timeout elapsed; cancellation remains in effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedStreamStopReport {
    pub executor_finished: bool,
    pub metrics: ManagedStreamMetricsSnapshot,
}

#[derive(Clone)]
pub struct ManagedAsyncStream {
    handle: Arc<Handle>,
}

struct Handle {
    runtime: Arc<Runtime>,
    executor: Mutex<Option<JoinHandle<()>>>,
}

struct Runtime {
    session_id: SessionId,
    options: ManagedStreamOptions,
    adapter: Arc<dyn ManagedStreamAdapter>,
    state: Mutex<MailboxState>,
    changed: Condvar,
    registrations: Mutex<HashMap<RequestId, Weak<TerminalState>>>,
    next_sequence: AtomicU64,
    lifecycle: AtomicU8,
    metrics: Metrics,
    executor_done: Mutex<bool>,
    executor_done_changed: Condvar,
}

struct MailboxState {
    input: VecDeque<Envelope>,
    worker_completions: VecDeque<WorkerCompletion>,
    results: VecDeque<StreamCompletion>,
    closed: bool,
}

struct Envelope {
    sequence: u64,
    request_id: RequestId,
    session_id: SessionId,
    input: Frame,
    deadline: Instant,
    attempt_limit: NonZeroUsize,
    terminal: Arc<TerminalState>,
}

struct TerminalState {
    cancelled: AtomicBool,
    terminal: AtomicBool,
    admission: Mutex<Option<AdmissionLease>>,
}

struct WorkerCompletion {
    sequence: u64,
    request_id: RequestId,
    result: Option<StreamResult>,
}

struct OrderedResolution {
    request_id: RequestId,
    result: Option<StreamResult>,
}

struct ActiveRequest {
    request_id: RequestId,
    deadline: Instant,
    terminal: Arc<TerminalState>,
}

#[derive(Default)]
struct Metrics {
    submitted: AtomicU64,
    accepted: AtomicU64,
    mailbox_full: AtomicU64,
    started: AtomicU64,
    attempts: AtomicU64,
    retries: AtomicU64,
    reconnects: AtomicU64,
    completed: AtomicU64,
    succeeded: AtomicU64,
    retry_exhausted: AtomicU64,
    failed: AtomicU64,
    timed_out: AtomicU64,
    cancelled: AtomicU64,
    late_results_discarded: AtomicU64,
    result_backpressure: AtomicU64,
    results_delivered: AtomicU64,
    queued_inputs: AtomicUsize,
    active_requests: AtomicUsize,
    ordered_pending: AtomicUsize,
    queued_results: AtomicUsize,
    peak_active_requests: AtomicUsize,
}

impl ManagedAsyncStream {
    pub fn new<A>(
        session_id: SessionId,
        options: ManagedStreamOptions,
        adapter: A,
    ) -> Result<Self, ManagedStreamBuildError>
    where
        A: ManagedStreamAdapter,
    {
        let runtime = Arc::new(Runtime {
            session_id,
            options,
            adapter: Arc::new(adapter),
            state: Mutex::new(MailboxState {
                input: VecDeque::new(),
                worker_completions: VecDeque::new(),
                results: VecDeque::new(),
                closed: false,
            }),
            changed: Condvar::new(),
            registrations: Mutex::new(HashMap::new()),
            next_sequence: AtomicU64::new(0),
            lifecycle: AtomicU8::new(0),
            metrics: Metrics::default(),
            executor_done: Mutex::new(false),
            executor_done_changed: Condvar::new(),
        });
        let executor_runtime = runtime.clone();
        let executor = thread::Builder::new()
            .name(runtime.options.thread_name.to_string())
            .spawn(move || run_executor(executor_runtime))
            .map_err(ManagedStreamBuildError)?;
        Ok(Self {
            handle: Arc::new(Handle {
                runtime,
                executor: Mutex::new(Some(executor)),
            }),
        })
    }

    /// Attempts submission without waiting for mailbox space or service I/O.
    pub fn try_submit(&self, request: AsyncRequest) -> SubmitOutcome {
        let runtime = &self.handle.runtime;
        runtime.metrics.submitted.fetch_add(1, Ordering::Relaxed);
        if request.session_id != runtime.session_id {
            return SubmitOutcome::Cancelled;
        }
        if runtime.lifecycle.load(Ordering::Acquire) != 0 {
            return SubmitOutcome::Stopping;
        }

        let mut mailbox = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
        if mailbox.closed {
            return SubmitOutcome::Stopping;
        }
        if mailbox.input.len() == runtime.options.input_capacity.get() {
            runtime.metrics.mailbox_full.fetch_add(1, Ordering::Relaxed);
            return SubmitOutcome::MailboxFull;
        }

        let mut registrations = runtime
            .registrations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if registrations
            .get(&request.request_id)
            .and_then(Weak::upgrade)
            .is_some()
        {
            return SubmitOutcome::Cancelled;
        }
        let terminal = Arc::new(TerminalState {
            cancelled: AtomicBool::new(false),
            terminal: AtomicBool::new(false),
            admission: Mutex::new(Some(request.admission)),
        });
        registrations.insert(request.request_id, Arc::downgrade(&terminal));
        let sequence = runtime.next_sequence.fetch_add(1, Ordering::Relaxed);
        mailbox.input.push_back(Envelope {
            sequence,
            request_id: request.request_id,
            session_id: request.session_id,
            input: request.input,
            deadline: request.deadline,
            attempt_limit: request.attempt_limit,
            terminal,
        });
        runtime.metrics.accepted.fetch_add(1, Ordering::Relaxed);
        runtime
            .metrics
            .queued_inputs
            .fetch_add(1, Ordering::Release);
        drop(registrations);
        runtime.changed.notify_one();
        SubmitOutcome::Accepted
    }

    /// Cancels a queued or active request and releases its admission lease.
    /// Any eventual adapter response is discarded.
    pub fn cancel(&self, request_id: RequestId) -> bool {
        let registration = self
            .handle
            .runtime
            .registrations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&request_id)
            .and_then(Weak::upgrade);
        let Some(terminal) = registration else {
            return false;
        };
        let cancelled = terminal.cancel(&self.handle.runtime.metrics);
        self.handle.runtime.changed.notify_all();
        cancelled
    }

    /// Receives one completed response without blocking.
    pub fn try_recv(&self) -> Option<StreamCompletion> {
        let runtime = &self.handle.runtime;
        let mut mailbox = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
        let result = mailbox.results.pop_front();
        if result.is_some() {
            runtime
                .metrics
                .queued_results
                .fetch_sub(1, Ordering::AcqRel);
            runtime
                .metrics
                .results_delivered
                .fetch_add(1, Ordering::Relaxed);
            runtime.changed.notify_one();
        }
        result
    }

    /// Waits for a result mailbox transition. Graph and capture threads should
    /// use [`Self::try_recv`]; this is intended for a dedicated dispatcher.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<StreamCompletion> {
        if let Some(result) = self.try_recv() {
            return Some(result);
        }
        let runtime = &self.handle.runtime;
        let mailbox = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
        let (mut mailbox, _) = runtime
            .changed
            .wait_timeout_while(mailbox, timeout, |state| {
                state.results.is_empty() && !state.closed
            })
            .unwrap_or_else(|e| e.into_inner());
        let result = mailbox.results.pop_front();
        if result.is_some() {
            runtime
                .metrics
                .queued_results
                .fetch_sub(1, Ordering::AcqRel);
            runtime
                .metrics
                .results_delivered
                .fetch_add(1, Ordering::Relaxed);
            runtime.changed.notify_one();
        }
        result
    }

    pub fn metrics(&self) -> ManagedStreamMetricsSnapshot {
        self.handle
            .runtime
            .metrics
            .snapshot(lifecycle(&self.handle.runtime), &self.handle.runtime.state)
    }

    /// Idempotently rejects new work, releases all admission leases, discards
    /// results, and waits only for the session executor (not blocked adapters).
    pub fn stop(&self, timeout: Duration) -> ManagedStreamStopReport {
        request_stop(&self.handle.runtime);
        let mut done = self
            .handle
            .runtime
            .executor_done
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !*done {
            let waited = self
                .handle
                .runtime
                .executor_done_changed
                .wait_timeout_while(done, timeout, |value| !*value)
                .unwrap_or_else(|e| e.into_inner());
            done = waited.0;
        }
        let executor_finished = *done;
        drop(done);
        if executor_finished {
            if let Some(executor) = self
                .handle
                .executor
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                let _ = executor.join();
            }
        }
        ManagedStreamStopReport {
            executor_finished,
            metrics: self.metrics(),
        }
    }
}

impl Drop for ManagedAsyncStream {
    fn drop(&mut self) {
        if Arc::strong_count(&self.handle) == 1 {
            request_stop(&self.handle.runtime);
        }
    }
}

impl TerminalState {
    fn finish(&self) -> bool {
        if self.terminal.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.admission
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        true
    }

    fn cancel(&self, metrics: &Metrics) -> bool {
        self.cancelled.store(true, Ordering::Release);
        if !self.finish() {
            return false;
        }
        metrics.cancelled.fetch_add(1, Ordering::Relaxed);
        metrics.completed.fetch_add(1, Ordering::Relaxed);
        true
    }
}

impl Metrics {
    fn snapshot(
        &self,
        state: ManagedStreamState,
        mailbox: &Mutex<MailboxState>,
    ) -> ManagedStreamMetricsSnapshot {
        let mailbox = mailbox.lock().unwrap_or_else(|e| e.into_inner());
        ManagedStreamMetricsSnapshot {
            state,
            submitted: self.submitted.load(Ordering::Acquire),
            accepted: self.accepted.load(Ordering::Acquire),
            mailbox_full: self.mailbox_full.load(Ordering::Acquire),
            started: self.started.load(Ordering::Acquire),
            attempts: self.attempts.load(Ordering::Acquire),
            retries: self.retries.load(Ordering::Acquire),
            reconnects: self.reconnects.load(Ordering::Acquire),
            completed: self.completed.load(Ordering::Acquire),
            succeeded: self.succeeded.load(Ordering::Acquire),
            retry_exhausted: self.retry_exhausted.load(Ordering::Acquire),
            failed: self.failed.load(Ordering::Acquire),
            timed_out: self.timed_out.load(Ordering::Acquire),
            cancelled: self.cancelled.load(Ordering::Acquire),
            late_results_discarded: self.late_results_discarded.load(Ordering::Acquire),
            result_backpressure: self.result_backpressure.load(Ordering::Acquire),
            results_delivered: self.results_delivered.load(Ordering::Acquire),
            queued_inputs: mailbox.input.len(),
            active_requests: self.active_requests.load(Ordering::Acquire),
            ordered_pending: self.ordered_pending.load(Ordering::Acquire),
            queued_results: mailbox.results.len(),
            peak_active_requests: self.peak_active_requests.load(Ordering::Acquire),
        }
    }
}

fn lifecycle(runtime: &Runtime) -> ManagedStreamState {
    match runtime.lifecycle.load(Ordering::Acquire) {
        0 => ManagedStreamState::Running,
        1 => ManagedStreamState::Stopping,
        _ => ManagedStreamState::Stopped,
    }
}

fn request_stop(runtime: &Arc<Runtime>) {
    if runtime
        .lifecycle
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    {
        let mut mailbox = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
        mailbox.closed = true;
        mailbox.input.clear();
        let completed_discarded = mailbox
            .worker_completions
            .iter()
            .filter(|completion| completion.result.is_some())
            .count() as u64;
        mailbox.worker_completions.clear();
        runtime.metrics.queued_inputs.store(0, Ordering::Release);
        let discarded = mailbox.results.len() as u64 + completed_discarded;
        mailbox.results.clear();
        runtime
            .metrics
            .late_results_discarded
            .fetch_add(discarded, Ordering::Relaxed);
        runtime.metrics.queued_results.store(0, Ordering::Release);
    }
    let terminals = {
        let mut registrations = runtime
            .registrations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let terminals = registrations
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        registrations.clear();
        terminals
    };
    for terminal in terminals {
        terminal.cancel(&runtime.metrics);
    }
    runtime.changed.notify_all();
}

fn run_executor(runtime: Arc<Runtime>) {
    let mut active = BTreeMap::<u64, ActiveRequest>::new();
    let mut ordered = BTreeMap::<u64, OrderedResolution>::new();
    let mut relaxed = VecDeque::<StreamCompletion>::new();
    let mut next_delivery = 0_u64;

    loop {
        let completions = {
            let mut mailbox = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
            if mailbox.closed {
                break;
            }
            mailbox.worker_completions.drain(..).collect::<Vec<_>>()
        };

        let had_completions = !completions.is_empty();
        for completion in completions {
            active.remove(&completion.sequence);
            runtime
                .metrics
                .active_requests
                .store(active.len(), Ordering::Release);
            remove_registration(&runtime, completion.request_id);
            match runtime.options.ordering {
                DeliveryOrdering::Strict => {
                    if completion.sequence >= next_delivery
                        && !ordered.contains_key(&completion.sequence)
                    {
                        ordered.insert(
                            completion.sequence,
                            OrderedResolution {
                                request_id: completion.request_id,
                                result: completion.result,
                            },
                        );
                    }
                }
                DeliveryOrdering::Relaxed => {
                    if let Some(result) = completion.result {
                        relaxed.push_back(StreamCompletion {
                            request_id: completion.request_id,
                            session_id: runtime.session_id.clone(),
                            result,
                        });
                    }
                }
            }
        }

        let now = Instant::now();
        for (sequence, request) in &active {
            if request.terminal.cancelled.load(Ordering::Acquire) {
                if runtime.options.ordering == DeliveryOrdering::Strict
                    && *sequence >= next_delivery
                    && !ordered.contains_key(sequence)
                {
                    ordered.insert(
                        *sequence,
                        OrderedResolution {
                            request_id: request.request_id,
                            result: None,
                        },
                    );
                }
            } else if now >= request.deadline && request.terminal.finish() {
                runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
                runtime.metrics.timed_out.fetch_add(1, Ordering::Relaxed);
                let result = StreamResult::Failed(ServiceError::timeout());
                match runtime.options.ordering {
                    DeliveryOrdering::Strict => {
                        ordered.entry(*sequence).or_insert(OrderedResolution {
                            request_id: request.request_id,
                            result: Some(result),
                        });
                    }
                    DeliveryOrdering::Relaxed => relaxed.push_back(StreamCompletion {
                        request_id: request.request_id,
                        session_id: runtime.session_id.clone(),
                        result,
                    }),
                }
            }
        }

        let mut launches = Vec::new();
        let mut made_progress = had_completions;
        {
            let mut mailbox = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
            if mailbox.closed {
                break;
            }

            while mailbox.results.len() < runtime.options.result_capacity.get() {
                let Some(result) = relaxed.pop_front() else {
                    break;
                };
                mailbox.results.push_back(result);
                runtime
                    .metrics
                    .queued_results
                    .fetch_add(1, Ordering::Release);
                made_progress = true;
            }
            if !relaxed.is_empty() && mailbox.results.len() == runtime.options.result_capacity.get()
            {
                runtime
                    .metrics
                    .result_backpressure
                    .fetch_add(1, Ordering::Relaxed);
            }

            if runtime.options.ordering == DeliveryOrdering::Strict {
                while let Some(resolution) = ordered.get(&next_delivery) {
                    if resolution.result.is_some()
                        && mailbox.results.len() == runtime.options.result_capacity.get()
                    {
                        runtime
                            .metrics
                            .result_backpressure
                            .fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                    let resolution = ordered.remove(&next_delivery).expect("entry exists");
                    if let Some(result) = resolution.result {
                        mailbox.results.push_back(StreamCompletion {
                            request_id: resolution.request_id,
                            session_id: runtime.session_id.clone(),
                            result,
                        });
                        runtime
                            .metrics
                            .queued_results
                            .fetch_add(1, Ordering::Release);
                    }
                    next_delivery += 1;
                    made_progress = true;
                }
            }

            runtime
                .metrics
                .ordered_pending
                .store(ordered.len() + relaxed.len(), Ordering::Release);

            loop {
                let buffered_not_active = ordered
                    .keys()
                    .filter(|sequence| !active.contains_key(sequence))
                    .count()
                    + relaxed.len();
                if active.len() + buffered_not_active >= runtime.options.max_in_flight.get() {
                    break;
                }
                let Some(envelope) = mailbox.input.pop_front() else {
                    break;
                };
                runtime.metrics.queued_inputs.fetch_sub(1, Ordering::AcqRel);
                made_progress = true;
                if envelope.terminal.cancelled.load(Ordering::Acquire) {
                    remove_registration(&runtime, envelope.request_id);
                    if runtime.options.ordering == DeliveryOrdering::Strict {
                        ordered.insert(
                            envelope.sequence,
                            OrderedResolution {
                                request_id: envelope.request_id,
                                result: None,
                            },
                        );
                    }
                    continue;
                }
                if Instant::now() >= envelope.deadline {
                    if envelope.terminal.finish() {
                        runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
                        runtime.metrics.timed_out.fetch_add(1, Ordering::Relaxed);
                    }
                    remove_registration(&runtime, envelope.request_id);
                    let result = StreamResult::Failed(ServiceError::timeout());
                    match runtime.options.ordering {
                        DeliveryOrdering::Strict => {
                            ordered.insert(
                                envelope.sequence,
                                OrderedResolution {
                                    request_id: envelope.request_id,
                                    result: Some(result),
                                },
                            );
                        }
                        DeliveryOrdering::Relaxed => relaxed.push_back(StreamCompletion {
                            request_id: envelope.request_id,
                            session_id: runtime.session_id.clone(),
                            result,
                        }),
                    }
                    continue;
                }
                active.insert(
                    envelope.sequence,
                    ActiveRequest {
                        request_id: envelope.request_id,
                        deadline: envelope.deadline,
                        terminal: envelope.terminal.clone(),
                    },
                );
                runtime.metrics.started.fetch_add(1, Ordering::Relaxed);
                runtime
                    .metrics
                    .active_requests
                    .store(active.len(), Ordering::Release);
                runtime
                    .metrics
                    .peak_active_requests
                    .fetch_max(active.len(), Ordering::Relaxed);
                launches.push(envelope);
            }

            if launches.is_empty() && !made_progress {
                let next_deadline = active
                    .values()
                    .filter(|request| !request.terminal.terminal.load(Ordering::Acquire))
                    .map(|request| request.deadline)
                    .min();
                if let Some(deadline) = next_deadline {
                    let wait = deadline.saturating_duration_since(Instant::now());
                    let _guard = runtime
                        .changed
                        .wait_timeout(mailbox, wait)
                        .unwrap_or_else(|e| e.into_inner());
                } else {
                    let _guard = runtime
                        .changed
                        .wait(mailbox)
                        .unwrap_or_else(|e| e.into_inner());
                }
                continue;
            }
        }

        for envelope in launches {
            spawn_request_worker(runtime.clone(), envelope);
        }
        runtime.changed.notify_all();
    }

    runtime.lifecycle.store(2, Ordering::Release);
    runtime.metrics.active_requests.store(0, Ordering::Release);
    let mut done = runtime
        .executor_done
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *done = true;
    runtime.executor_done_changed.notify_all();
    runtime.changed.notify_all();
}

fn spawn_request_worker(runtime: Arc<Runtime>, envelope: Envelope) {
    let sequence = envelope.sequence;
    let request_id = envelope.request_id;
    let terminal = envelope.terminal.clone();
    let worker_runtime = runtime.clone();
    let spawn = thread::Builder::new()
        .name(format!("{}-request", runtime.options.thread_name))
        .spawn(move || run_request(worker_runtime, envelope));
    if spawn.is_err() {
        let won = terminal.finish();
        if won {
            runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
            runtime.metrics.failed.fetch_add(1, Ordering::Relaxed);
        }
        let result = won.then(|| StreamResult::Failed(ServiceError::worker_start()));
        push_worker_completion(
            &runtime,
            WorkerCompletion {
                sequence,
                request_id,
                result,
            },
        );
    }
}

fn run_request(runtime: Arc<Runtime>, envelope: Envelope) {
    let mut terminal_result = None;
    for attempt in 1..=envelope.attempt_limit.get() {
        if envelope.terminal.cancelled.load(Ordering::Acquire) {
            break;
        }
        if Instant::now() >= envelope.deadline {
            terminal_result = Some(StreamResult::Failed(ServiceError::timeout()));
            break;
        }
        runtime.metrics.attempts.fetch_add(1, Ordering::Relaxed);
        let reconnecting = attempt > 1;
        if reconnecting {
            runtime.metrics.reconnects.fetch_add(1, Ordering::Relaxed);
            if !runtime.options.reconnect_delay.is_zero() {
                thread::sleep(runtime.options.reconnect_delay);
            }
            if envelope.terminal.cancelled.load(Ordering::Acquire) {
                break;
            }
            if Instant::now() >= envelope.deadline {
                terminal_result = Some(StreamResult::Failed(ServiceError::timeout()));
                break;
            }
        }
        let response = catch_unwind(AssertUnwindSafe(|| {
            match runtime.adapter.connect(&envelope.session_id, reconnecting) {
                Ok(()) => runtime.adapter.send(AdapterRequest {
                    request_id: envelope.request_id,
                    session_id: envelope.session_id.clone(),
                    input: envelope.input.clone(),
                    attempt,
                }),
                Err(error) => AdapterResponse::Retryable(error),
            }
        }))
        .unwrap_or_else(|_| AdapterResponse::Failed(ServiceError::adapter_panic()));
        if envelope.terminal.cancelled.load(Ordering::Acquire) {
            break;
        }
        if Instant::now() >= envelope.deadline {
            terminal_result = Some(StreamResult::Failed(ServiceError::timeout()));
            break;
        }
        match response {
            AdapterResponse::Frames(frames) => {
                terminal_result = Some(StreamResult::Frames(frames));
                break;
            }
            AdapterResponse::Failed(error) => {
                terminal_result = Some(StreamResult::Failed(error));
                break;
            }
            AdapterResponse::Retryable(_error) if attempt < envelope.attempt_limit.get() => {
                runtime.metrics.retries.fetch_add(1, Ordering::Relaxed);
            }
            AdapterResponse::Retryable(error) => {
                terminal_result = Some(StreamResult::Retryable(error));
                break;
            }
        }
    }

    let won = envelope.terminal.finish();
    let result = if won {
        runtime.metrics.completed.fetch_add(1, Ordering::Relaxed);
        match &terminal_result {
            Some(StreamResult::Frames(_)) => {
                runtime.metrics.succeeded.fetch_add(1, Ordering::Relaxed);
            }
            Some(StreamResult::Retryable(_)) => {
                runtime
                    .metrics
                    .retry_exhausted
                    .fetch_add(1, Ordering::Relaxed);
            }
            Some(StreamResult::Failed(error)) if error.code() == "managed_stream_timeout" => {
                runtime.metrics.timed_out.fetch_add(1, Ordering::Relaxed);
            }
            Some(StreamResult::Failed(_)) | None => {
                runtime.metrics.failed.fetch_add(1, Ordering::Relaxed);
            }
        }
        terminal_result
    } else {
        runtime
            .metrics
            .late_results_discarded
            .fetch_add(1, Ordering::Relaxed);
        None
    };
    push_worker_completion(
        &runtime,
        WorkerCompletion {
            sequence: envelope.sequence,
            request_id: envelope.request_id,
            result,
        },
    );
}

fn push_worker_completion(runtime: &Runtime, completion: WorkerCompletion) {
    let mut mailbox = runtime.state.lock().unwrap_or_else(|e| e.into_inner());
    if !mailbox.closed {
        mailbox.worker_completions.push_back(completion);
        runtime.changed.notify_one();
    } else if completion.result.is_some() {
        runtime
            .metrics
            .late_results_discarded
            .fetch_add(1, Ordering::Relaxed);
    }
}

fn remove_registration(runtime: &Runtime, request_id: RequestId) {
    runtime
        .registrations
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&request_id);
}
