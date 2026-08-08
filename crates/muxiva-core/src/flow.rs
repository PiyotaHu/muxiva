//! Per-input-port adaptive pressure prediction and explicit overload actions.
//!
//! The controller is deliberately standalone in Stage 5B. Stage 5C binds its
//! admission/completion observations to managed service terminal outcomes;
//! Stage 6 may translate its bounded signal observations into `SignalFrame`s.

use std::{
    collections::{BTreeSet, VecDeque},
    sync::Arc,
    time::Duration,
};

use muxiva_types::NodeId;

use crate::{
    AudioOverflowPolicy, DeliveryGuarantee, PortName, RealtimeInputProfile, RealtimeProfileError,
};

const EMA_ALPHA: f64 = 0.25;
const RESUME_SAMPLES: u8 = 3;
const SIGNAL_HISTORY_LIMIT: usize = 64;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InputPortKey {
    pub node_id: NodeId,
    pub port: PortName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowState {
    Normal,
    Pressure,
    Critical,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameMeasurement {
    pub bytes: usize,
    pub media_duration: Duration,
}

impl FrameMeasurement {
    pub const fn new(bytes: usize, media_duration: Duration) -> Self {
        Self {
            bytes,
            media_duration,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlowSnapshot {
    pub key: InputPortKey,
    pub state: FlowState,
    pub queued_frames: usize,
    pub queued_bytes: usize,
    pub queued_media_duration: Duration,
    pub in_flight: usize,
    pub input_media_duration_total: Duration,
    pub completed_media_duration_total: Duration,
    pub service_time_ema: Duration,
    pub predicted_latency: Duration,
    pub predicted_time_to_saturation: Option<Duration>,
    pub dropped_frames_total: u64,
    pub dropped_bytes_total: u64,
    pub dropped_media_duration_total: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowAction {
    KeepNormalBatch,
    SetBatchTarget(Duration),
    LimitAdmissions(usize),
    ResumeAdmissions,
    EmitPressure,
    EmitResume,
    ApplyOverflow(AudioOverflowPolicy),
    AbortSession,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowSignalObservation {
    Pressure,
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustedVadDecision {
    Silence,
    Speech,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverflowDecision {
    PropagateBackpressure,
    DropOldest,
    DropNewest,
    DropTrustedSilence,
    AbortSession,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowDropReason {
    FrameLimit,
    ByteLimit,
    MediaDurationLimit,
    TrustedVadSilence,
    PressureShedding,
    DeadlineExpired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlowUpdate {
    pub snapshot: FlowSnapshot,
    pub actions: Vec<FlowAction>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowError {
    InvalidProfile(RealtimeProfileError),
    ClockMovedBackwards,
    CounterOverflow,
    AdmissionWithoutQueuedFrame,
    CompletionWithoutAdmission,
    DropWithoutQueuedFrame,
    LosslessDropForbidden,
}

pub trait FlowClock: Send + Sync {
    fn now(&self) -> Duration;
}

/// An admitted item retained until its processing completion is observed.
///
/// A work item is intentionally not cloneable: it is the unique completion
/// capability for one specific admission on one controller.
///
/// ```compile_fail
/// # use muxiva_core::FlowWork;
/// # fn duplicate(work: FlowWork) {
/// let duplicate = work.clone();
/// # }
/// ```
pub struct FlowWork {
    controller: Arc<()>,
    admission_id: u64,
    measurement: FrameMeasurement,
    started_at: Duration,
}

pub struct AdaptiveFlowController {
    key: InputPortKey,
    profile: RealtimeInputProfile,
    clock: Arc<dyn FlowClock>,
    identity: Arc<()>,
    next_admission_id: u64,
    active_admissions: BTreeSet<u64>,
    state: FlowState,
    queued_frames: usize,
    queued_bytes: usize,
    queued_media: Duration,
    in_flight: usize,
    in_flight_bytes: usize,
    in_flight_media: Duration,
    input_media_total: Duration,
    completed_media_total: Duration,
    input_rate: Rates,
    completion_rate: Rates,
    last_input_at: Option<Duration>,
    service_time_ema_secs: f64,
    healthy_samples: u8,
    signals: VecDeque<FlowSignalObservation>,
    dropped_frames: u64,
    dropped_bytes: u64,
    dropped_media: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
struct Rates {
    frames_per_sec: f64,
    bytes_per_sec: f64,
    media_per_wall: f64,
}

impl AdaptiveFlowController {
    pub fn new(
        key: InputPortKey,
        profile: RealtimeInputProfile,
        clock: Arc<dyn FlowClock>,
    ) -> Result<Self, FlowError> {
        profile.validate().map_err(FlowError::InvalidProfile)?;
        Ok(Self {
            key,
            profile,
            clock,
            identity: Arc::new(()),
            next_admission_id: 0,
            active_admissions: BTreeSet::new(),
            state: FlowState::Normal,
            queued_frames: 0,
            queued_bytes: 0,
            queued_media: Duration::ZERO,
            in_flight: 0,
            in_flight_bytes: 0,
            in_flight_media: Duration::ZERO,
            input_media_total: Duration::ZERO,
            completed_media_total: Duration::ZERO,
            input_rate: Rates::default(),
            completion_rate: Rates::default(),
            last_input_at: None,
            service_time_ema_secs: 0.0,
            healthy_samples: 0,
            signals: VecDeque::new(),
            dropped_frames: 0,
            dropped_bytes: 0,
            dropped_media: Duration::ZERO,
        })
    }

    pub const fn profile(&self) -> &RealtimeInputProfile {
        &self.profile
    }

    pub fn record_enqueue(
        &mut self,
        measurement: FrameMeasurement,
    ) -> Result<FlowUpdate, FlowError> {
        let now = self.clock.now();
        if let Some(previous) = self.last_input_at {
            let elapsed = now
                .checked_sub(previous)
                .ok_or(FlowError::ClockMovedBackwards)?;
            if !elapsed.is_zero() {
                self.input_rate.observe(measurement, elapsed);
            }
        }
        self.last_input_at = Some(now);
        self.queued_frames = self
            .queued_frames
            .checked_add(1)
            .ok_or(FlowError::CounterOverflow)?;
        self.queued_bytes = self
            .queued_bytes
            .checked_add(measurement.bytes)
            .ok_or(FlowError::CounterOverflow)?;
        self.queued_media = self
            .queued_media
            .checked_add(measurement.media_duration)
            .ok_or(FlowError::CounterOverflow)?;
        self.input_media_total = self
            .input_media_total
            .checked_add(measurement.media_duration)
            .ok_or(FlowError::CounterOverflow)?;
        Ok(self.evaluate())
    }

    pub fn record_admission(
        &mut self,
        measurement: FrameMeasurement,
    ) -> Result<(FlowWork, FlowUpdate), FlowError> {
        if self.queued_frames == 0
            || self.queued_bytes < measurement.bytes
            || self.queued_media < measurement.media_duration
        {
            return Err(FlowError::AdmissionWithoutQueuedFrame);
        }
        let admission_id = self.next_admission_id;
        self.next_admission_id = self
            .next_admission_id
            .checked_add(1)
            .ok_or(FlowError::CounterOverflow)?;
        self.queued_frames -= 1;
        self.queued_bytes -= measurement.bytes;
        self.queued_media -= measurement.media_duration;
        self.in_flight = self
            .in_flight
            .checked_add(1)
            .ok_or(FlowError::CounterOverflow)?;
        self.in_flight_bytes = self
            .in_flight_bytes
            .checked_add(measurement.bytes)
            .ok_or(FlowError::CounterOverflow)?;
        self.in_flight_media = self
            .in_flight_media
            .checked_add(measurement.media_duration)
            .ok_or(FlowError::CounterOverflow)?;
        if !self.active_admissions.insert(admission_id) {
            return Err(FlowError::CounterOverflow);
        }
        let work = FlowWork {
            controller: self.identity.clone(),
            admission_id,
            measurement,
            started_at: self.clock.now(),
        };
        Ok((work, self.evaluate()))
    }

    pub fn record_completion(&mut self, work: FlowWork) -> Result<FlowUpdate, FlowError> {
        if !Arc::ptr_eq(&self.identity, &work.controller)
            || !self.active_admissions.remove(&work.admission_id)
            || self.in_flight == 0
            || self.in_flight_bytes < work.measurement.bytes
            || self.in_flight_media < work.measurement.media_duration
        {
            return Err(FlowError::CompletionWithoutAdmission);
        }
        let now = self.clock.now();
        let service_time = now
            .checked_sub(work.started_at)
            .ok_or(FlowError::ClockMovedBackwards)?;
        if !service_time.is_zero() {
            self.completion_rate.observe(work.measurement, service_time);
            self.service_time_ema_secs =
                ema(self.service_time_ema_secs, service_time.as_secs_f64());
        }
        self.in_flight -= 1;
        self.in_flight_bytes -= work.measurement.bytes;
        self.in_flight_media -= work.measurement.media_duration;
        self.completed_media_total = self
            .completed_media_total
            .checked_add(work.measurement.media_duration)
            .ok_or(FlowError::CounterOverflow)?;
        Ok(self.evaluate())
    }

    pub fn record_drop(
        &mut self,
        measurement: FrameMeasurement,
        _reason: FlowDropReason,
    ) -> Result<FlowUpdate, FlowError> {
        if self.profile.contract.delivery_guarantee == DeliveryGuarantee::Lossless {
            return Err(FlowError::LosslessDropForbidden);
        }
        if self.queued_frames == 0
            || self.queued_bytes < measurement.bytes
            || self.queued_media < measurement.media_duration
        {
            return Err(FlowError::DropWithoutQueuedFrame);
        }
        self.queued_frames -= 1;
        self.queued_bytes -= measurement.bytes;
        self.queued_media -= measurement.media_duration;
        self.dropped_frames = self
            .dropped_frames
            .checked_add(1)
            .ok_or(FlowError::CounterOverflow)?;
        self.dropped_bytes = self
            .dropped_bytes
            .checked_add(u64::try_from(measurement.bytes).map_err(|_| FlowError::CounterOverflow)?)
            .ok_or(FlowError::CounterOverflow)?;
        self.dropped_media = self
            .dropped_media
            .checked_add(measurement.media_duration)
            .ok_or(FlowError::CounterOverflow)?;
        Ok(self.evaluate())
    }

    /// Selects a validated critical action. Silence is droppable only from a
    /// typed runtime VAD observation, never from caller metadata.
    pub fn decide_overflow(&self, vad: Option<TrustedVadDecision>) -> OverflowDecision {
        debug_assert!(self.profile.validate().is_ok());
        match self.profile.tuning.overflow_policy {
            AudioOverflowPolicy::PropagateBackpressure => OverflowDecision::PropagateBackpressure,
            AudioOverflowPolicy::DropOldest => OverflowDecision::DropOldest,
            AudioOverflowPolicy::DropNewest => OverflowDecision::DropNewest,
            AudioOverflowPolicy::DropSilenceFirst => match vad {
                Some(TrustedVadDecision::Silence) => OverflowDecision::DropTrustedSilence,
                _ if self.profile.contract.upstream_can_pause => {
                    OverflowDecision::PropagateBackpressure
                }
                _ => OverflowDecision::AbortSession,
            },
            AudioOverflowPolicy::AbortSession => OverflowDecision::AbortSession,
        }
    }

    pub fn snapshot(&self) -> FlowSnapshot {
        self.snapshot_with_prediction(self.predict())
    }

    /// Takes a control sample without inventing queue or service work.
    pub fn sample(&mut self) -> FlowUpdate {
        if self.last_input_at.is_some_and(|last_input| {
            self.clock.now().saturating_sub(last_input)
                >= self.profile.tuning.deadline.saturating_mul(2)
        }) {
            self.input_rate = Rates::default();
        }
        self.evaluate()
    }

    pub fn signal_observations(&self) -> impl Iterator<Item = FlowSignalObservation> + '_ {
        self.signals.iter().copied()
    }

    fn evaluate(&mut self) -> FlowUpdate {
        let prediction = self.predict();
        let tuning = self.profile.tuning.clone();
        let contract = self.profile.contract.clone();
        let frame_ratio = ratio(self.queued_frames, tuning.max_frames);
        let byte_ratio = ratio(self.queued_bytes, tuning.max_bytes);
        let media_ratio = duration_ratio(self.queued_media, tuning.max_buffered_media_duration);
        let capacity_ratio = frame_ratio.max(byte_ratio).max(media_ratio);
        let latency_ratio = duration_ratio(prediction.latency, contract.latency_budget);
        let hard_limit = frame_ratio >= 1.0 || byte_ratio >= 1.0 || media_ratio >= 1.0;
        let control_interval = tuning.deadline / 2;
        let saturation_critical = prediction
            .time_to_saturation
            .is_some_and(|time| time <= control_interval);
        let saturation_pressure = prediction
            .time_to_saturation
            .is_some_and(|time| time <= tuning.deadline);
        let desired = if hard_limit || latency_ratio >= 1.0 || saturation_critical {
            FlowState::Critical
        } else if capacity_ratio >= 0.75 || latency_ratio >= 0.75 || saturation_pressure {
            FlowState::Pressure
        } else {
            FlowState::Normal
        };

        let healthy = capacity_ratio < 0.5
            && latency_ratio < 0.5
            && prediction
                .time_to_saturation
                .is_none_or(|time| time > tuning.deadline.saturating_mul(2));
        let previous = self.state;
        match (self.state, desired) {
            (FlowState::Normal, next) => self.state = next,
            (FlowState::Pressure, FlowState::Critical) => self.state = FlowState::Critical,
            (FlowState::Pressure | FlowState::Critical, FlowState::Normal) if healthy => {
                self.healthy_samples = self.healthy_samples.saturating_add(1);
                if self.healthy_samples >= RESUME_SAMPLES {
                    self.state = FlowState::Normal;
                    self.healthy_samples = 0;
                }
            }
            (FlowState::Pressure | FlowState::Critical, _) => self.healthy_samples = 0,
        }

        let emit_resume = previous != self.state && self.state == FlowState::Normal;
        let emit_pressure = previous == FlowState::Normal && self.state != FlowState::Normal;
        if emit_resume {
            self.record_signal(FlowSignalObservation::Resume);
        } else if emit_pressure {
            self.record_signal(FlowSignalObservation::Pressure);
        }
        let mut actions = Vec::new();
        match self.state {
            FlowState::Normal => {
                actions.push(FlowAction::KeepNormalBatch);
                actions.push(FlowAction::ResumeAdmissions);
                if emit_resume {
                    actions.push(FlowAction::EmitResume);
                }
            }
            FlowState::Pressure => {
                if let Some(maximum) = tuning.max_merge_duration {
                    actions.push(FlowAction::SetBatchTarget(maximum));
                }
                actions.push(FlowAction::LimitAdmissions(
                    (tuning.max_in_flight / 2).max(1),
                ));
                if emit_pressure {
                    actions.push(FlowAction::EmitPressure);
                }
            }
            FlowState::Critical => {
                actions.push(FlowAction::LimitAdmissions(0));
                if emit_pressure {
                    actions.push(FlowAction::EmitPressure);
                }
                match tuning.overflow_policy {
                    AudioOverflowPolicy::AbortSession => actions.push(FlowAction::AbortSession),
                    policy => actions.push(FlowAction::ApplyOverflow(policy)),
                }
            }
        }
        FlowUpdate {
            snapshot: self.snapshot_with_prediction(prediction),
            actions,
        }
    }

    fn record_signal(&mut self, signal: FlowSignalObservation) {
        if !self.profile.contract.upstream_can_pause {
            return;
        }
        if self.signals.len() == SIGNAL_HISTORY_LIMIT {
            self.signals.pop_front();
        }
        self.signals.push_back(signal);
    }

    fn predict(&self) -> Prediction {
        let backlog_media = self.queued_media.saturating_add(self.in_flight_media);
        let backlog_frames = self.queued_frames.saturating_add(self.in_flight);
        let latency_secs =
            if !backlog_media.is_zero() && self.completion_rate.media_per_wall > f64::EPSILON {
                backlog_media.as_secs_f64() / self.completion_rate.media_per_wall
                    + self.service_time_ema_secs
            } else if backlog_frames > 0 && self.service_time_ema_secs > 0.0 {
                backlog_frames as f64 * self.service_time_ema_secs
                    / self.profile.tuning.max_in_flight as f64
            } else {
                0.0
            };

        let mut saturation_secs: Option<f64> = None;
        consider_saturation(
            &mut saturation_secs,
            self.profile
                .tuning
                .max_frames
                .saturating_sub(self.queued_frames) as f64,
            self.input_rate.frames_per_sec - self.completion_rate.frames_per_sec,
        );
        consider_saturation(
            &mut saturation_secs,
            self.profile
                .tuning
                .max_bytes
                .saturating_sub(self.queued_bytes) as f64,
            self.input_rate.bytes_per_sec - self.completion_rate.bytes_per_sec,
        );
        consider_saturation(
            &mut saturation_secs,
            self.profile
                .tuning
                .max_buffered_media_duration
                .saturating_sub(self.queued_media)
                .as_secs_f64(),
            self.input_rate.media_per_wall - self.completion_rate.media_per_wall,
        );
        Prediction {
            latency: duration_from_secs(latency_secs),
            time_to_saturation: saturation_secs.map(duration_from_secs),
        }
    }

    fn snapshot_with_prediction(&self, prediction: Prediction) -> FlowSnapshot {
        FlowSnapshot {
            key: self.key.clone(),
            state: self.state,
            queued_frames: self.queued_frames,
            queued_bytes: self.queued_bytes,
            queued_media_duration: self.queued_media,
            in_flight: self.in_flight,
            input_media_duration_total: self.input_media_total,
            completed_media_duration_total: self.completed_media_total,
            service_time_ema: duration_from_secs(self.service_time_ema_secs),
            predicted_latency: prediction.latency,
            predicted_time_to_saturation: prediction.time_to_saturation,
            dropped_frames_total: self.dropped_frames,
            dropped_bytes_total: self.dropped_bytes,
            dropped_media_duration_total: self.dropped_media,
        }
    }
}

impl Rates {
    fn observe(&mut self, measurement: FrameMeasurement, elapsed: Duration) {
        let seconds = elapsed.as_secs_f64();
        self.frames_per_sec = ema(self.frames_per_sec, 1.0 / seconds);
        self.bytes_per_sec = ema(self.bytes_per_sec, measurement.bytes as f64 / seconds);
        self.media_per_wall = ema(
            self.media_per_wall,
            measurement.media_duration.as_secs_f64() / seconds,
        );
    }
}

#[derive(Clone, Copy)]
struct Prediction {
    latency: Duration,
    time_to_saturation: Option<Duration>,
}

fn ema(current: f64, sample: f64) -> f64 {
    if current == 0.0 {
        sample
    } else {
        EMA_ALPHA * sample + (1.0 - EMA_ALPHA) * current
    }
}

fn consider_saturation(current: &mut Option<f64>, remaining: f64, net_rate: f64) {
    if net_rate <= f64::EPSILON {
        return;
    }
    let candidate = remaining / net_rate;
    *current = Some(current.map_or(candidate, |value| value.min(candidate)));
}

fn ratio(value: usize, limit: usize) -> f64 {
    value as f64 / limit as f64
}

fn duration_ratio(value: Duration, limit: Duration) -> f64 {
    value.as_secs_f64() / limit.as_secs_f64()
}

fn duration_from_secs(seconds: f64) -> Duration {
    if !seconds.is_finite() || seconds >= Duration::MAX.as_secs_f64() {
        Duration::MAX
    } else if seconds <= 0.0 {
        Duration::ZERO
    } else {
        Duration::from_secs_f64(seconds)
    }
}

/// Defensive assertion used by overflow integrations handling lossless ports.
pub fn overflow_may_drop(profile: &RealtimeInputProfile) -> bool {
    profile.contract.delivery_guarantee != DeliveryGuarantee::Lossless
        && profile.tuning.overflow_policy.drops_frames()
}
