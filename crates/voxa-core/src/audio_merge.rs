//! Compatible audio-prefix coalescing immediately before admission.

use std::time::Duration;

use voxa_types::{
    AudioData, AudioLayout, ErrorCategory, Frame, FrameBuffer, FrameId, TransformOrigin, VoxaError,
};

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
) -> voxa_types::Result<Option<MergedAudioFrame>> {
    if target.is_zero() || maximum.is_zero() || target > maximum {
        return Err(VoxaError::new(
            ErrorCategory::Validation,
            "VOXA-RT-AUDIO-MERGE-DURATION",
            "audio merge target must be non-zero and no greater than its maximum",
        ));
    }
    let Some(first) = frames.first().and_then(Frame::as_audio) else {
        return Ok(None);
    };
    let mut source_count = 0_usize;
    let mut duration = Duration::ZERO;
    for frame in frames.iter().take(1_024) {
        let Some(audio) = frame.as_audio() else {
            break;
        };
        if source_count > 0 && !compatible_with_previous(&frames[source_count - 1], frame) {
            break;
        }
        let next_duration = duration
            .checked_add(Duration::from_nanos(audio.data().duration_ns()))
            .ok_or_else(merge_arithmetic_error)?;
        if next_duration > maximum {
            break;
        }
        duration = next_duration;
        source_count += 1;
        if duration >= target {
            break;
        }
    }
    if source_count < 2 || duration < target {
        return Ok(None);
    }

    let parents = &frames[..source_count];
    let bytes = merge_payload_bytes(parents, first.data().layout())?;
    let samples_per_channel = parents.iter().try_fold(0_u64, |sum, frame| {
        sum.checked_add(
            frame
                .as_audio()
                .expect("compatible prefix contains audio")
                .data()
                .samples_per_channel(),
        )
        .ok_or_else(merge_arithmetic_error)
    })?;
    let merged_data = AudioData::new(
        FrameBuffer::from_vec(bytes),
        first.data().sample_rate_hz(),
        first.data().channels(),
        first.data().sample_format(),
        first.data().layout(),
        samples_per_channel,
    )?;
    let frame = Frame::merge_audio(parents, id_source.next_frame_id(), merged_data, origin)?;
    Ok(Some(MergedAudioFrame {
        frame,
        source_count,
    }))
}

fn compatible_with_previous(previous: &Frame, next: &Frame) -> bool {
    let (Some(previous_audio), Some(next_audio)) = (previous.as_audio(), next.as_audio()) else {
        return false;
    };
    let previous_header = previous.header();
    let next_header = next.header();
    let Ok(duration_ns) = i64::try_from(previous_audio.data().duration_ns()) else {
        return false;
    };
    previous_header.stream_id() == next_header.stream_id()
        && previous_header.trace_id() == next_header.trace_id()
        && previous_header.clock_domain() == next_header.clock_domain()
        && previous_header.sequence_id().checked_next() == Some(next_header.sequence_id())
        && previous_header.timestamp().checked_add(duration_ns) == Some(next_header.timestamp())
        && previous_audio.data().sample_rate_hz() == next_audio.data().sample_rate_hz()
        && previous_audio.data().channels() == next_audio.data().channels()
        && previous_audio.data().sample_format() == next_audio.data().sample_format()
        && previous_audio.data().layout() == next_audio.data().layout()
}

fn merge_payload_bytes(parents: &[Frame], layout: AudioLayout) -> voxa_types::Result<Vec<u8>> {
    let total_bytes = parents.iter().try_fold(0_usize, |sum, frame| {
        sum.checked_add(
            frame
                .as_audio()
                .expect("compatible prefix contains audio")
                .data()
                .buffer()
                .len(),
        )
        .ok_or_else(merge_arithmetic_error)
    })?;
    let mut output = Vec::with_capacity(total_bytes);
    match layout {
        AudioLayout::Interleaved => {
            for frame in parents {
                output.extend_from_slice(
                    frame
                        .as_audio()
                        .expect("compatible prefix contains audio")
                        .data()
                        .buffer()
                        .as_slice(),
                );
            }
        }
        AudioLayout::Planar => {
            let channels = parents[0]
                .as_audio()
                .expect("compatible prefix contains audio")
                .data()
                .channels();
            for channel in 0..channels {
                for frame in parents {
                    output.extend_from_slice(
                        frame
                            .as_audio()
                            .expect("compatible prefix contains audio")
                            .data()
                            .plane_bytes(channel)?,
                    );
                }
            }
        }
    }
    Ok(output)
}

fn merge_arithmetic_error() -> VoxaError {
    VoxaError::new(
        ErrorCategory::Validation,
        "VOXA-RT-AUDIO-MERGE-ARITHMETIC",
        "audio merge arithmetic overflowed",
    )
}
