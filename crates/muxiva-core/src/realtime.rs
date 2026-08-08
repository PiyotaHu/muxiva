//! Declarative realtime behavior and bounded runtime input tuning.

use std::{fmt, time::Duration};

/// Whether loss at a realtime input is permitted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DeliveryGuarantee {
    Lossless,
    BestEffort,
}

/// Ordering required for admitted work and results.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DeliveryOrdering {
    Strict,
    Relaxed,
}

/// Declared terminal behavior after adaptive measures cannot contain overload.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioOverflowPolicy {
    PropagateBackpressure,
    DropOldest,
    DropNewest,
    DropSilenceFirst,
    AbortSession,
}

impl AudioOverflowPolicy {
    pub const fn drops_frames(self) -> bool {
        matches!(
            self,
            Self::DropOldest | Self::DropNewest | Self::DropSilenceFirst
        )
    }
}

/// Inclusive duration bounds accepted by an audio input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioDurationRange {
    pub min: Duration,
    pub max: Duration,
}

/// Stable business declaration, separate from runtime measurements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeContract {
    pub latency_budget: Duration,
    pub delivery_guarantee: DeliveryGuarantee,
    pub permits_audio: bool,
    pub accepted_audio_duration: Option<AudioDurationRange>,
    pub ordering: DeliveryOrdering,
    pub upstream_can_pause: bool,
    pub permits_trusted_vad_silence_drop: bool,
}

/// Internal bounded capacity, coalescing, admission, and deadline choices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeInputTuning {
    pub max_frames: usize,
    pub max_bytes: usize,
    pub max_buffered_media_duration: Duration,
    pub target_merge_duration: Option<Duration>,
    pub max_merge_duration: Option<Duration>,
    pub max_in_flight: usize,
    pub deadline: Duration,
    pub overflow_policy: AudioOverflowPolicy,
}

/// Fully resolved, externally inspectable input-port profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeInputProfile {
    pub contract: RealtimeContract,
    pub tuning: RuntimeInputTuning,
}

/// One stable reason a realtime input profile was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeProfileError {
    ZeroLatencyBudget,
    LatencyBudgetUnbounded,
    AudioRangeRequired,
    AudioRangeForbidden,
    InvalidAudioRange,
    ZeroCapacity,
    CapacityUnbounded,
    ZeroMediaDurationLimit,
    MediaDurationLimitUnbounded,
    MergeDurationRequired,
    MergeDurationForbidden,
    InvalidMergeDuration,
    ZeroDeadline,
    DeadlineUnbounded,
    DeadlineExceedsLatencyBudget,
    LosslessDrop,
    StrictOrderingDrop,
    SilenceDropNotPermitted,
    UnpausableBackpressureOnly,
}

impl RealtimeProfileError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ZeroLatencyBudget => "MUXIVA-RT-LATENCY-ZERO",
            Self::LatencyBudgetUnbounded => "MUXIVA-RT-LATENCY-UNBOUNDED",
            Self::AudioRangeRequired => "MUXIVA-RT-AUDIO-RANGE-REQUIRED",
            Self::AudioRangeForbidden => "MUXIVA-RT-AUDIO-RANGE-FORBIDDEN",
            Self::InvalidAudioRange => "MUXIVA-RT-AUDIO-RANGE",
            Self::ZeroCapacity => "MUXIVA-RT-CAPACITY-ZERO",
            Self::CapacityUnbounded => "MUXIVA-RT-CAPACITY-UNBOUNDED",
            Self::ZeroMediaDurationLimit => "MUXIVA-RT-MEDIA-LIMIT-ZERO",
            Self::MediaDurationLimitUnbounded => "MUXIVA-RT-MEDIA-LIMIT-UNBOUNDED",
            Self::MergeDurationRequired => "MUXIVA-RT-MERGE-REQUIRED",
            Self::MergeDurationForbidden => "MUXIVA-RT-MERGE-FORBIDDEN",
            Self::InvalidMergeDuration => "MUXIVA-RT-MERGE-RANGE",
            Self::ZeroDeadline => "MUXIVA-RT-DEADLINE-ZERO",
            Self::DeadlineUnbounded => "MUXIVA-RT-DEADLINE-UNBOUNDED",
            Self::DeadlineExceedsLatencyBudget => "MUXIVA-RT-DEADLINE-BUDGET",
            Self::LosslessDrop => "MUXIVA-RT-LOSSLESS-DROP",
            Self::StrictOrderingDrop => "MUXIVA-RT-STRICT-DROP",
            Self::SilenceDropNotPermitted => "MUXIVA-RT-SILENCE-DROP",
            Self::UnpausableBackpressureOnly => "MUXIVA-RT-UNPAUSABLE-BACKPRESSURE",
        }
    }
}

impl fmt::Display for RealtimeProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for RealtimeProfileError {}

impl RealtimeInputProfile {
    /// Validates business permissions and bounded runtime choices together.
    pub fn validate(&self) -> Result<(), RealtimeProfileError> {
        const MAX_REALTIME_DURATION: Duration = Duration::from_secs(60 * 60);
        const MAX_FRAMES: usize = 1_000_000;
        const MAX_BYTES: usize = 1 << 40;

        let contract = &self.contract;
        let tuning = &self.tuning;
        if contract.latency_budget.is_zero() {
            return Err(RealtimeProfileError::ZeroLatencyBudget);
        }
        if contract.latency_budget > MAX_REALTIME_DURATION {
            return Err(RealtimeProfileError::LatencyBudgetUnbounded);
        }
        match (contract.permits_audio, contract.accepted_audio_duration) {
            (true, None) => return Err(RealtimeProfileError::AudioRangeRequired),
            (false, Some(_)) => return Err(RealtimeProfileError::AudioRangeForbidden),
            (true, Some(range))
                if range.min.is_zero() || range.max.is_zero() || range.min > range.max =>
            {
                return Err(RealtimeProfileError::InvalidAudioRange);
            }
            _ => {}
        }
        if tuning.max_frames == 0 || tuning.max_bytes == 0 || tuning.max_in_flight == 0 {
            return Err(RealtimeProfileError::ZeroCapacity);
        }
        if tuning.max_frames > MAX_FRAMES
            || tuning.max_bytes > MAX_BYTES
            || tuning.max_in_flight > tuning.max_frames
        {
            return Err(RealtimeProfileError::CapacityUnbounded);
        }
        if tuning.max_buffered_media_duration.is_zero() {
            return Err(RealtimeProfileError::ZeroMediaDurationLimit);
        }
        if tuning.max_buffered_media_duration > MAX_REALTIME_DURATION {
            return Err(RealtimeProfileError::MediaDurationLimitUnbounded);
        }
        match (
            contract.permits_audio,
            tuning.target_merge_duration,
            tuning.max_merge_duration,
        ) {
            (true, None, _) | (true, _, None) => {
                return Err(RealtimeProfileError::MergeDurationRequired);
            }
            (false, None, None) => {}
            (false, _, _) => return Err(RealtimeProfileError::MergeDurationForbidden),
            (true, Some(target), Some(maximum)) => {
                let range = contract.accepted_audio_duration.expect("validated range");
                if target.is_zero()
                    || maximum.is_zero()
                    || target > maximum
                    || target < range.min
                    || maximum > range.max
                    || maximum > tuning.max_buffered_media_duration
                {
                    return Err(RealtimeProfileError::InvalidMergeDuration);
                }
            }
        }
        if tuning.deadline.is_zero() {
            return Err(RealtimeProfileError::ZeroDeadline);
        }
        if tuning.deadline > MAX_REALTIME_DURATION {
            return Err(RealtimeProfileError::DeadlineUnbounded);
        }
        if tuning.deadline > contract.latency_budget {
            return Err(RealtimeProfileError::DeadlineExceedsLatencyBudget);
        }
        if contract.delivery_guarantee == DeliveryGuarantee::Lossless
            && tuning.overflow_policy.drops_frames()
        {
            return Err(RealtimeProfileError::LosslessDrop);
        }
        if contract.ordering == DeliveryOrdering::Strict && tuning.overflow_policy.drops_frames() {
            return Err(RealtimeProfileError::StrictOrderingDrop);
        }
        if tuning.overflow_policy == AudioOverflowPolicy::DropSilenceFirst
            && (contract.delivery_guarantee != DeliveryGuarantee::BestEffort
                || !contract.permits_audio
                || !contract.permits_trusted_vad_silence_drop)
        {
            return Err(RealtimeProfileError::SilenceDropNotPermitted);
        }
        if !contract.upstream_can_pause
            && tuning.overflow_policy == AudioOverflowPolicy::PropagateBackpressure
        {
            return Err(RealtimeProfileError::UnpausableBackpressureOnly);
        }
        Ok(())
    }
}

impl Default for RealtimeInputProfile {
    /// A conservative non-audio, lossless, pausable profile with one slot.
    fn default() -> Self {
        Self {
            contract: RealtimeContract {
                latency_budget: Duration::from_millis(250),
                delivery_guarantee: DeliveryGuarantee::Lossless,
                permits_audio: false,
                accepted_audio_duration: None,
                ordering: DeliveryOrdering::Strict,
                upstream_can_pause: true,
                permits_trusted_vad_silence_drop: false,
            },
            tuning: RuntimeInputTuning {
                max_frames: 8,
                max_bytes: 1024 * 1024,
                max_buffered_media_duration: Duration::from_millis(250),
                target_merge_duration: None,
                max_merge_duration: None,
                max_in_flight: 1,
                deadline: Duration::from_millis(250),
                overflow_policy: AudioOverflowPolicy::PropagateBackpressure,
            },
        }
    }
}
