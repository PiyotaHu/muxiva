//! Compatible audio-prefix coalescing immediately before admission.

use std::time::Duration;

use muxiva_types::{ErrorCategory, Frame, FrameId, MuxivaError, Timestamp, TransformOrigin};

/// Supplies fresh frame IDs without deriving identity from timing or memory addresses.
pub trait FrameIdSource {
    fn next_frame_id(&mut self) -> FrameId;
}

/// One merged frame and the number of atomic queue residents it consumes.
#[derive(Clone, Debug)]
pub struct MergedAudioFrame {
    frame: Frame,
    source_count: usize,
}

impl MergedAudioFrame {
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    pub const fn source_count(&self) -> usize {
        self.source_count
    }

    pub fn into_frame(self) -> Frame {
        self.frame
    }
}

/// Merges the longest compatible prefix that reaches `target` without
/// exceeding `maximum`. An incompatible/non-audio prefix remains untouched.
pub fn merge_audio_prefix(
    frames: &[Frame],
    target: Duration,
    maximum: Duration,
    id_source: &mut impl FrameIdSource,
    origin: TransformOrigin,
) -> muxiva_types::Result<Option<MergedAudioFrame>> {
    if target.is_zero() || maximum.is_zero() || target > maximum {
        return Err(MuxivaError::new(
            ErrorCategory::Validation,
            "MUXIVA-RT-AUDIO-MERGE-DURATION",
            "audio merge target must be non-zero and no greater than its maximum",
        ));
    }
    let Some(first) = frames.first().and_then(Frame::as_audio) else {
        return Ok(None);
    };
    let mut source_count = 0_usize;
    let mut samples_per_channel = 0_u64;
    for frame in frames.iter().take(1_024) {
        let Some(audio) = frame.as_audio() else {
            break;
        };
        let expected_timestamp = sample_boundary_timestamp(
            first.header().timestamp(),
            samples_per_channel,
            first.data().sample_rate_hz(),
        )?;
        if source_count > 0
            && !compatible_with_previous(&frames[source_count - 1], frame, expected_timestamp)
        {
            break;
        }
        let next_samples = samples_per_channel
            .checked_add(audio.data().samples_per_channel())
            .ok_or_else(merge_arithmetic_error)?;
        let next_duration = Duration::from_nanos(duration_ns_for_samples(
            next_samples,
            first.data().sample_rate_hz(),
        )?);
        if next_duration > maximum {
            break;
        }
        samples_per_channel = next_samples;
        source_count += 1;
        if next_duration >= target {
            break;
        }
    }
    let duration = Duration::from_nanos(duration_ns_for_samples(
        samples_per_channel,
        first.data().sample_rate_hz(),
    )?);
    if source_count < 2 || duration < target {
        return Ok(None);
    }

    let parents = &frames[..source_count];
    let frame = Frame::merge_audio(parents, id_source.next_frame_id(), origin)?;
    Ok(Some(MergedAudioFrame {
        frame,
        source_count,
    }))
}

fn compatible_with_previous(previous: &Frame, next: &Frame, expected_timestamp: Timestamp) -> bool {
    let (Some(previous_audio), Some(next_audio)) = (previous.as_audio(), next.as_audio()) else {
        return false;
    };
    let previous_header = previous.header();
    let next_header = next.header();
    previous_header.stream_id() == next_header.stream_id()
        && previous_header.trace_id() == next_header.trace_id()
        && previous_header.clock_domain() == next_header.clock_domain()
        && previous_header.sequence_id().checked_next() == Some(next_header.sequence_id())
        && next_header.timestamp() == expected_timestamp
        && previous_audio.data().sample_rate_hz() == next_audio.data().sample_rate_hz()
        && previous_audio.data().channels() == next_audio.data().channels()
        && previous_audio.data().sample_format() == next_audio.data().sample_format()
        && previous_audio.data().layout() == next_audio.data().layout()
}

fn sample_boundary_timestamp(
    start: Timestamp,
    samples: u64,
    sample_rate_hz: u32,
) -> muxiva_types::Result<Timestamp> {
    let offset = duration_ns_for_samples(samples, sample_rate_hz)?;
    start
        .checked_add(i64::try_from(offset).map_err(|_| merge_arithmetic_error())?)
        .ok_or_else(merge_arithmetic_error)
}

fn duration_ns_for_samples(samples: u64, sample_rate_hz: u32) -> muxiva_types::Result<u64> {
    let rate = u64::from(sample_rate_hz);
    let whole_seconds = samples / rate;
    let remaining_samples = samples % rate;
    whole_seconds
        .checked_mul(1_000_000_000)
        .and_then(|whole| {
            remaining_samples
                .checked_mul(1_000_000_000)
                .and_then(|fraction| whole.checked_add(fraction / rate))
        })
        .ok_or_else(merge_arithmetic_error)
}

fn merge_arithmetic_error() -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Validation,
        "MUXIVA-RT-AUDIO-MERGE-ARITHMETIC",
        "audio merge arithmetic overflowed",
    )
}
