//! Bounded, language-neutral command/completion gate for foreign node domains.
//!
//! A Python or JavaScript binding owns its interpreter objects and execution
//! thread. Core only gives it this owned-data driver: a bounded command mailbox,
//! bounded completion mailbox, cancellation controls, deadlines, and the one
//! authority that publishes an abort reason. Bindings must never call a graph
//! worker or an Edge queue directly.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    num::NonZeroUsize,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use voxa_types::{EventFrame, Frame, SignalFrame, Value};

use crate::{AbortReason, StopToken};

const MAX_MAILBOX_ITEMS: usize = 65_536;
const MAX_MAILBOX_BYTES: usize = 64 * 1024 * 1024;
const MAX_IN_FLIGHT: usize = 4_096;
const MAX_DURATION: Duration = Duration::from_secs(60 * 60);
const DIAGNOSTIC_LIMIT: usize = 256;

/// Required result when a bounded foreign mailbox cannot accept more work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForeignFullPolicy {
    /// Reject immediately. A caller must apply its declared Edge/profile policy.
    Reject,
}

/// Declared completion release ordering for one foreign node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForeignOrdering {
    /// Release only the next submitted sequence after every prior sequence.
    Strict,
    /// Release a completed sequence as soon as it is accepted.
    Unordered,
}

/// Bounded, language-neutral driver settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForeignDriverConfig {
    pub command_capacity: NonZeroUsize,
    pub command_byte_capacity: NonZeroUsize,
    pub completion_capacity: NonZeroUsize,
    pub completion_byte_capacity: NonZeroUsize,
    pub max_in_flight: NonZeroUsize,
    pub per_call_deadline: Duration,
    pub shutdown_deadline: Duration,
    pub ordering: ForeignOrdering,
    pub command_full_policy: ForeignFullPolicy,
    pub completion_full_policy: ForeignFullPolicy,
}

impl Default for ForeignDriverConfig {
    fn default() -> Self {
        Self {
            command_capacity: NonZeroUsize::new(16).expect("constant is non-zero"),
            command_byte_capacity: NonZeroUsize::new(1 << 20).expect("constant is non-zero"),
            completion_capacity: NonZeroUsize::new(16).expect("constant is non-zero"),
            completion_byte_capacity: NonZeroUsize::new(1 << 20).expect("constant is non-zero"),
            max_in_flight: NonZeroUsize::MIN,
            per_call_deadline: Duration::from_secs(10),
            shutdown_deadline: Duration::from_secs(5),
            ordering: ForeignOrdering::Strict,
            command_full_policy: ForeignFullPolicy::Reject,
            completion_full_policy: ForeignFullPolicy::Reject,
        }
    }
}

impl ForeignDriverConfig {
    pub fn validate(self) -> Result<(), ForeignDriverError> {
        for capacity in [
            self.command_capacity.get(),
            self.completion_capacity.get(),
            self.max_in_flight.get(),
        ] {
            if capacity > MAX_MAILBOX_ITEMS || self.max_in_flight.get() > MAX_IN_FLIGHT {
                return Err(ForeignDriverError::CapacityTooLarge);
            }
        }
        if self.command_byte_capacity.get() > MAX_MAILBOX_BYTES
            || self.completion_byte_capacity.get() > MAX_MAILBOX_BYTES
        {
            return Err(ForeignDriverError::ByteCapacityTooLarge);
        }
        if self.per_call_deadline.is_zero() || self.shutdown_deadline.is_zero() {
            return Err(ForeignDriverError::ZeroDeadline);
        }
        if self.per_call_deadline > MAX_DURATION || self.shutdown_deadline > MAX_DURATION {
            return Err(ForeignDriverError::DeadlineTooLarge);
        }
        Ok(())
    }
}

/// One owned command delivered into a foreign execution domain.
#[derive(Clone, Eq, PartialEq)]
pub struct ForeignCommand {
    sequence: u64,
    kind: ForeignCommandKind,
}

/// The only values which cross into a foreign domain.
#[derive(Clone, Eq, PartialEq)]
pub enum ForeignCommandKind {
    Prepare,
    Process(Frame),
    Signal(SignalFrame),
    Event(EventFrame),
    Finish,
    /// Cancels one already-dispatched asynchronous call.
    Cancel,
    Abort(AbortReason),
    Stop,
}

impl ForeignCommand {
    pub fn new(sequence: u64, kind: ForeignCommandKind) -> Self {
        Self { sequence, kind }
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn kind(&self) -> &ForeignCommandKind {
        &self.kind
    }
}

/// One owned completion returned by a foreign execution domain.
#[derive(Clone, Eq, PartialEq)]
pub struct ForeignCompletion {
    sequence: u64,
    kind: ForeignCompletionKind,
}

/// A successful, cancelled, or structured-failure foreign completion.
#[derive(Clone, Eq, PartialEq)]
pub enum ForeignCompletionKind {
    Success {
        frames: Box<[Frame]>,
        signals: Box<[SignalFrame]>,
        events: Box<[EventFrame]>,
    },
    Failure(AbortReason),
}

impl ForeignCompletion {
    pub fn success(
        sequence: u64,
        frames: impl Into<Box<[Frame]>>,
        signals: impl Into<Box<[SignalFrame]>>,
        events: impl Into<Box<[EventFrame]>>,
    ) -> Self {
        Self {
            sequence,
            kind: ForeignCompletionKind::Success {
                frames: frames.into(),
                signals: signals.into(),
                events: events.into(),
            },
        }
    }

    pub fn failure(sequence: u64, reason: AbortReason) -> Self {
        Self {
            sequence,
            kind: ForeignCompletionKind::Failure(reason),
        }
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn kind(&self) -> &ForeignCompletionKind {
        &self.kind
    }
}

/// A submit result that never makes a producer wait on a foreign interpreter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForeignSubmitOutcome {
    Accepted,
    Full,
    Closed,
    Cancelled,
}

/// A completion result. Late output is deliberately observable and discarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForeignCompletionOutcome {
    Accepted,
    Full,
    Closed,
    LateDiscarded,
}

/// Configuration or state errors from a foreign driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForeignDriverError {
    CapacityTooLarge,
    ByteCapacityTooLarge,
    ZeroDeadline,
    DeadlineTooLarge,
    DuplicateSequence,
    StrictSequenceGap,
}

impl fmt::Display for ForeignDriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CapacityTooLarge => "foreign mailbox item capacity exceeds the Core hard limit",
            Self::ByteCapacityTooLarge => {
                "foreign mailbox byte capacity exceeds the Core hard limit"
            }
            Self::ZeroDeadline => "foreign execution deadlines must be non-zero",
            Self::DeadlineTooLarge => "foreign execution deadlines exceed the Core hard limit",
            Self::DuplicateSequence => "foreign command sequence is already live",
            Self::StrictSequenceGap => "strict foreign command sequence must be contiguous",
        })
    }
}

impl std::error::Error for ForeignDriverError {}

/// Bounded diagnostics returned after a shutdown deadline instead of a forever join.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignShutdownDiagnostics {
    live_sequences: Box<[u64]>,
    queued_command_count: usize,
    queued_completion_count: usize,
}

impl ForeignShutdownDiagnostics {
    pub fn live_sequences(&self) -> &[u64] {
        &self.live_sequences
    }

    pub const fn queued_command_count(&self) -> usize {
        self.queued_command_count
    }

    pub const fn queued_completion_count(&self) -> usize {
        self.queued_completion_count
    }
}

/// Read-only bounded mailbox and lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForeignDriverSnapshot {
    pub command_count: usize,
    pub command_bytes: usize,
    pub completion_count: usize,
    pub completion_bytes: usize,
    pub in_flight: usize,
    pub accepting: bool,
    pub stopping: bool,
    pub late_completion_total: u64,
    pub cancellation_total: u64,
}

/// Core-owned foreign-domain driver. It contains no interpreter, callback, or
/// borrowed foreign value.
#[derive(Clone)]
pub struct ForeignNodeDriver {
    shared: Arc<Shared>,
}

struct Shared {
    config: ForeignDriverConfig,
    stop: StopToken,
    state: Mutex<State>,
    changed: Condvar,
}

struct State {
    commands: VecDeque<QueuedCommand>,
    command_bytes: usize,
    controls: VecDeque<ForeignCommand>,
    ready: VecDeque<QueuedCompletion>,
    ready_bytes: usize,
    ordered: BTreeMap<u64, QueuedCompletion>,
    ordered_bytes: usize,
    live: BTreeMap<u64, LiveCommand>,
    strict_order: VecDeque<u64>,
    last_strict_sequence: Option<u64>,
    accepting: bool,
    stopping: bool,
    abort_reason: Option<AbortReason>,
    abort_taken: bool,
    late_completion_total: u64,
    cancellation_total: u64,
}

struct QueuedCommand {
    command: ForeignCommand,
    bytes: usize,
}

struct QueuedCompletion {
    completion: ForeignCompletion,
    bytes: usize,
}

struct LiveCommand {
    deadline: Instant,
    dispatched: bool,
    cancelled: bool,
    completion_received: bool,
}

impl ForeignNodeDriver {
    pub fn new(config: ForeignDriverConfig) -> Result<Self, ForeignDriverError> {
        config.validate()?;
        Ok(Self {
            shared: Arc::new(Shared {
                config,
                stop: StopToken::new(),
                state: Mutex::new(State {
                    commands: VecDeque::new(),
                    command_bytes: 0,
                    controls: VecDeque::new(),
                    ready: VecDeque::new(),
                    ready_bytes: 0,
                    ordered: BTreeMap::new(),
                    ordered_bytes: 0,
                    live: BTreeMap::new(),
                    strict_order: VecDeque::new(),
                    last_strict_sequence: None,
                    accepting: true,
                    stopping: false,
                    abort_reason: None,
                    abort_taken: false,
                    late_completion_total: 0,
                    cancellation_total: 0,
                }),
                changed: Condvar::new(),
            }),
        })
    }

    /// Submits owned work without waiting for an interpreter or callback thread.
    pub fn try_submit(
        &self,
        command: ForeignCommand,
        now: Instant,
    ) -> Result<ForeignSubmitOutcome, ForeignDriverError> {
        let bytes = command_bytes(&command);
        let mut state = lock(&self.shared.state);
        if state.stopping {
            return Ok(ForeignSubmitOutcome::Closed);
        }
        if self.shared.stop.is_cancelled() {
            return Ok(ForeignSubmitOutcome::Cancelled);
        }
        if !state.accepting {
            return Ok(ForeignSubmitOutcome::Closed);
        }
        if state.live.contains_key(&command.sequence) {
            return Err(ForeignDriverError::DuplicateSequence);
        }
        if self.shared.config.ordering == ForeignOrdering::Strict {
            if let Some(previous) = state.last_strict_sequence {
                if command.sequence != previous.checked_add(1).unwrap_or(previous) {
                    return Err(ForeignDriverError::StrictSequenceGap);
                }
            }
            state.last_strict_sequence = Some(command.sequence);
        }
        if state.live.len() == self.shared.config.max_in_flight.get()
            || state.commands.len() == self.shared.config.command_capacity.get()
            || !fits(
                state.command_bytes,
                bytes,
                self.shared.config.command_byte_capacity.get(),
            )
        {
            return Ok(ForeignSubmitOutcome::Full);
        }

        let sequence = command.sequence;
        state.commands.push_back(QueuedCommand { command, bytes });
        state.command_bytes = state.command_bytes.saturating_add(bytes);
        state.live.insert(
            sequence,
            LiveCommand {
                deadline: now + self.shared.config.per_call_deadline,
                dispatched: false,
                cancelled: false,
                completion_received: false,
            },
        );
        if self.shared.config.ordering == ForeignOrdering::Strict {
            state.strict_order.push_back(sequence);
        }
        self.shared.changed.notify_all();
        Ok(ForeignSubmitOutcome::Accepted)
    }

    /// Receives a cancellation/stop control before ordinary domain work.
    pub fn try_receive(&self) -> Option<ForeignCommand> {
        let mut state = lock(&self.shared.state);
        if let Some(control) = state.controls.pop_front() {
            return Some(control);
        }
        let queued = state.commands.pop_front()?;
        state.command_bytes = state.command_bytes.saturating_sub(queued.bytes);
        if let Some(live) = state.live.get_mut(&queued.command.sequence) {
            live.dispatched = true;
        }
        self.shared.changed.notify_all();
        Some(queued.command)
    }

    /// Accepts a completion without executing graph routing on the foreign thread.
    pub fn try_complete(&self, completion: ForeignCompletion) -> ForeignCompletionOutcome {
        let bytes = completion_bytes(&completion);
        let mut state = lock(&self.shared.state);
        let Some(live) = state.live.get(&completion.sequence) else {
            if state.stopping {
                return ForeignCompletionOutcome::Closed;
            }
            state.late_completion_total = state.late_completion_total.saturating_add(1);
            return ForeignCompletionOutcome::LateDiscarded;
        };
        if live.cancelled || state.stopping || self.shared.stop.is_cancelled() {
            state.late_completion_total = state.late_completion_total.saturating_add(1);
            state.live.remove(&completion.sequence);
            remove_sequence(&mut state.strict_order, completion.sequence);
            self.shared.changed.notify_all();
            return ForeignCompletionOutcome::LateDiscarded;
        }
        if live.completion_received {
            state.late_completion_total = state.late_completion_total.saturating_add(1);
            return ForeignCompletionOutcome::LateDiscarded;
        }

        if let ForeignCompletionKind::Failure(reason) = completion.kind() {
            let reason = reason.clone();
            state.live.remove(&completion.sequence);
            remove_sequence(&mut state.strict_order, completion.sequence);
            fail_locked(&self.shared, &mut state, reason);
            self.shared.changed.notify_all();
            return ForeignCompletionOutcome::Accepted;
        }

        let completion_count = state.ready.len() + state.ordered.len();
        let completion_bytes = state.ready_bytes.saturating_add(state.ordered_bytes);
        if completion_count == self.shared.config.completion_capacity.get()
            || !fits(
                completion_bytes,
                bytes,
                self.shared.config.completion_byte_capacity.get(),
            )
        {
            return ForeignCompletionOutcome::Full;
        }

        let queued = QueuedCompletion { completion, bytes };
        state
            .live
            .get_mut(&queued.completion.sequence)
            .expect("completion sequence was checked above")
            .completion_received = true;
        match self.shared.config.ordering {
            ForeignOrdering::Unordered => {
                state.ready_bytes = state.ready_bytes.saturating_add(queued.bytes);
                state.ready.push_back(queued);
            }
            ForeignOrdering::Strict => {
                state.ordered_bytes = state.ordered_bytes.saturating_add(queued.bytes);
                state.ordered.insert(queued.completion.sequence, queued);
                promote_strict(&mut state);
            }
        }
        self.shared.changed.notify_all();
        ForeignCompletionOutcome::Accepted
    }

    /// Takes a completion which Core may route through the normal Edge path.
    ///
    /// This method is intentionally separate from try_complete, so a foreign
    /// domain cannot synchronously call downstream Nodes or EventBus handlers.
    pub fn try_take_completion(&self) -> Option<ForeignCompletion> {
        let mut state = lock(&self.shared.state);
        let queued = state.ready.pop_front()?;
        state.ready_bytes = state.ready_bytes.saturating_sub(queued.bytes);
        state.live.remove(&queued.completion.sequence);
        remove_sequence(&mut state.strict_order, queued.completion.sequence);
        if self.shared.config.ordering == ForeignOrdering::Strict {
            promote_strict(&mut state);
        }
        self.shared.changed.notify_all();
        Some(queued.completion)
    }

    /// Expires live calls, seals future admission, and queues cancellation controls.
    pub fn expire_deadlines(&self, now: Instant) -> usize {
        let mut state = lock(&self.shared.state);
        let expired = state
            .live
            .iter()
            .filter_map(|(sequence, live)| (live.deadline <= now).then_some(*sequence))
            .collect::<Vec<_>>();
        if expired.is_empty() {
            return 0;
        }
        let reason = foreign_abort("VOXA-FOREIGN-DEADLINE", "foreign call deadline elapsed");
        fail_locked(&self.shared, &mut state, reason);
        state.cancellation_total = state
            .cancellation_total
            .saturating_add(u64::try_from(expired.len()).unwrap_or(u64::MAX));
        self.shared.changed.notify_all();
        expired.len()
    }

    /// Starts terminal cancellation. Only the first caller obtains authority.
    pub fn begin_stop(&self, reason: AbortReason) -> bool {
        let mut state = lock(&self.shared.state);
        if state.stopping {
            return false;
        }
        fail_locked(&self.shared, &mut state, reason);
        self.shared.changed.notify_all();
        true
    }

    /// Seals an idle domain after its terminal lifecycle callback completed.
    /// Returns false when work is still live or shutdown already started.
    pub fn begin_graceful_stop(&self) -> bool {
        let mut state = lock(&self.shared.state);
        if state.stopping || !state.live.is_empty() {
            return false;
        }
        state.accepting = false;
        state.stopping = true;
        self.shared.stop.cancel();
        state
            .controls
            .push_back(ForeignCommand::new(u64::MAX, ForeignCommandKind::Stop));
        self.shared.changed.notify_all();
        true
    }

    /// Returns the terminal reason exactly once for the runtime abort owner.
    pub fn take_abort_reason(&self) -> Option<AbortReason> {
        let mut state = lock(&self.shared.state);
        if state.abort_taken {
            return None;
        }
        let reason = state.abort_reason.clone()?;
        state.abort_taken = true;
        Some(reason)
    }

    /// A foreign domain acknowledges that its cancelled task has stopped.
    pub fn acknowledge_cancel(&self, sequence: u64) -> bool {
        let mut state = lock(&self.shared.state);
        let Some(live) = state.live.get(&sequence) else {
            return false;
        };
        if !live.cancelled {
            return false;
        }
        state.live.remove(&sequence);
        remove_sequence(&mut state.strict_order, sequence);
        self.shared.changed.notify_all();
        true
    }

    /// Waits without busy polling for active foreign tasks to reach a terminal state.
    pub fn wait_drained(&self, deadline: Instant) -> Result<(), ForeignShutdownDiagnostics> {
        let mut state = lock(&self.shared.state);
        while !state.live.is_empty() {
            let now = Instant::now();
            if now >= deadline {
                return Err(diagnostics(&state));
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next, timeout) = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
            if timeout.timed_out() && !state.live.is_empty() {
                return Err(diagnostics(&state));
            }
        }
        Ok(())
    }

    pub fn snapshot(&self) -> ForeignDriverSnapshot {
        let state = lock(&self.shared.state);
        ForeignDriverSnapshot {
            command_count: state.commands.len(),
            command_bytes: state.command_bytes,
            completion_count: state.ready.len() + state.ordered.len(),
            completion_bytes: state.ready_bytes.saturating_add(state.ordered_bytes),
            in_flight: state.live.len(),
            accepting: state.accepting,
            stopping: state.stopping,
            late_completion_total: state.late_completion_total,
            cancellation_total: state.cancellation_total,
        }
    }

    pub fn stop_token(&self) -> StopToken {
        self.shared.stop.clone()
    }
}

fn fail_locked(shared: &Shared, state: &mut State, reason: AbortReason) {
    state.accepting = false;
    state.stopping = true;
    shared.stop.cancel();
    if state.abort_reason.is_none() {
        state.abort_reason = Some(reason);
    }

    let undispatched = state
        .live
        .iter()
        .filter_map(|(sequence, live)| (!live.dispatched).then_some(*sequence))
        .collect::<Vec<_>>();
    state
        .commands
        .retain(|queued| !undispatched.contains(&queued.command.sequence));
    state.command_bytes = state.commands.iter().map(|queued| queued.bytes).sum();

    for sequence in undispatched {
        state.live.remove(&sequence);
        remove_sequence(&mut state.strict_order, sequence);
    }
    for (sequence, live) in &mut state.live {
        if !live.cancelled {
            live.cancelled = true;
            state
                .controls
                .push_back(ForeignCommand::new(*sequence, ForeignCommandKind::Cancel));
            state.cancellation_total = state.cancellation_total.saturating_add(1);
        }
    }
    state.controls.push_back(ForeignCommand::new(
        u64::MAX - 1,
        ForeignCommandKind::Abort(
            state
                .abort_reason
                .clone()
                .expect("terminal reason was assigned above"),
        ),
    ));
    state
        .controls
        .push_back(ForeignCommand::new(u64::MAX, ForeignCommandKind::Stop));
    state.ready.clear();
    state.ready_bytes = 0;
    state.ordered.clear();
    state.ordered_bytes = 0;
}

fn promote_strict(state: &mut State) {
    while let Some(sequence) = state.strict_order.front().copied() {
        let Some(queued) = state.ordered.remove(&sequence) else {
            break;
        };
        state.ordered_bytes = state.ordered_bytes.saturating_sub(queued.bytes);
        state.ready_bytes = state.ready_bytes.saturating_add(queued.bytes);
        state.ready.push_back(queued);
    }
}

fn remove_sequence(order: &mut VecDeque<u64>, sequence: u64) {
    if let Some(index) = order.iter().position(|candidate| *candidate == sequence) {
        order.remove(index);
    }
}

fn diagnostics(state: &State) -> ForeignShutdownDiagnostics {
    ForeignShutdownDiagnostics {
        live_sequences: state.live.keys().copied().collect(),
        queued_command_count: state.commands.len(),
        queued_completion_count: state.ready.len() + state.ordered.len(),
    }
}

fn foreign_abort(code: &str, message: &str) -> AbortReason {
    AbortReason::new(
        crate::AbortCategory::ForeignException,
        None,
        crate::AbortStage::Runtime,
        crate::AbortRootContext::new(code, message, crate::ConfigMap::empty()),
    )
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn fits(current: usize, addition: usize, capacity: usize) -> bool {
    current
        .checked_add(addition)
        .is_some_and(|total| total <= capacity)
}

fn command_bytes(command: &ForeignCommand) -> usize {
    match command.kind() {
        ForeignCommandKind::Process(frame) => frame_bytes(frame),
        ForeignCommandKind::Signal(frame) => signal_bytes(frame),
        ForeignCommandKind::Event(frame) => event_bytes(frame),
        ForeignCommandKind::Abort(reason) => reason_bytes(reason),
        ForeignCommandKind::Prepare
        | ForeignCommandKind::Finish
        | ForeignCommandKind::Cancel
        | ForeignCommandKind::Stop => 1,
    }
}

fn completion_bytes(completion: &ForeignCompletion) -> usize {
    match completion.kind() {
        ForeignCompletionKind::Success {
            frames,
            signals,
            events,
        } => frames
            .iter()
            .map(frame_bytes)
            .chain(signals.iter().map(signal_bytes))
            .chain(events.iter().map(event_bytes))
            .fold(1_usize, usize::saturating_add),
        ForeignCompletionKind::Failure(reason) => reason_bytes(reason),
    }
}

fn frame_bytes(frame: &Frame) -> usize {
    let payload = match frame {
        Frame::Audio(audio) => audio.data().buffer().len(),
        Frame::Video(video) => video.data().buffer().len(),
        Frame::Text(text) => text.data().as_str().len(),
        Frame::Byte(bytes) => bytes.data().buffer().len(),
        Frame::Signal(signal) => value_bytes(signal.data().payload()),
        Frame::Event(event) => value_bytes(event.data().payload()),
    };
    let header = frame
        .header()
        .metadata()
        .iter()
        .map(|(key, value)| key.len().saturating_add(value_bytes(value)))
        .chain(frame.header().extensions().iter().map(|extension| {
            extension
                .key()
                .as_str()
                .len()
                .saturating_add(value_bytes(extension.value()))
        }))
        .fold(64_usize, usize::saturating_add);
    payload.saturating_add(header)
}

fn signal_bytes(frame: &SignalFrame) -> usize {
    64_usize
        .saturating_add(frame.data().name().as_str().len())
        .saturating_add(frame.data().source().as_str().len())
        .saturating_add(value_bytes(frame.data().payload()))
}

fn event_bytes(frame: &EventFrame) -> usize {
    64_usize
        .saturating_add(frame.data().topic().as_str().len())
        .saturating_add(frame.data().source().as_str().len())
        .saturating_add(value_bytes(frame.data().payload()))
}

fn value_bytes(value: &Value) -> usize {
    match value {
        Value::Null | Value::Bool(_) | Value::Integer(_) | Value::Float(_) => 8,
        Value::String(string) => string.len(),
        Value::Bytes(bytes) => bytes.len(),
        Value::List(values) => values
            .iter()
            .map(value_bytes)
            .fold(8, usize::saturating_add),
        Value::Map(values) => values
            .iter()
            .map(|(key, value)| key.len().saturating_add(value_bytes(value)))
            .fold(8, usize::saturating_add),
    }
}

fn reason_bytes(reason: &AbortReason) -> usize {
    reason
        .root()
        .code()
        .len()
        .saturating_add(reason.root().message().len())
        .clamp(1, DIAGNOSTIC_LIMIT)
}
