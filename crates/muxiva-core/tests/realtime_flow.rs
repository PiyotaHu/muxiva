use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use muxiva_core::{
    merge_audio_prefix, overflow_may_drop, AdaptiveFlowController, AdmissionSlots,
    AudioDurationRange, AudioOverflowPolicy, DeliveryGuarantee, DeliveryOrdering, FlowAction,
    FlowClock, FlowSignalObservation, FlowState, FrameIdSource, FrameMeasurement, InputPortKey,
    OverflowDecision, PortName, RealtimeContract, RealtimeInputProfile, RealtimeProfileError,
    RuntimeInputTuning, TrustedVadDecision,
};
use muxiva_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, Extensions, Frame, FrameBuffer,
    FrameHeader, FrameId, FramePayload, FrameType, Lineage, Metadata, NodeId, PcmSampleFormat,
    SequenceId, StreamId, Timestamp, TraceId, TransformOrigin,
};

fn audio_profile(policy: AudioOverflowPolicy, pausable: bool) -> RealtimeInputProfile {
    let dropping = policy.drops_frames();
    RealtimeInputProfile {
        contract: RealtimeContract {
            latency_budget: Duration::from_millis(500),
            delivery_guarantee: if dropping {
                DeliveryGuarantee::BestEffort
            } else {
                DeliveryGuarantee::Lossless
            },
            permits_audio: true,
            accepted_audio_duration: Some(AudioDurationRange {
                min: Duration::from_millis(20),
                max: Duration::from_millis(100),
            }),
            ordering: if dropping {
                DeliveryOrdering::Relaxed
            } else {
                DeliveryOrdering::Strict
            },
            upstream_can_pause: pausable,
            permits_trusted_vad_silence_drop: policy == AudioOverflowPolicy::DropSilenceFirst,
        },
        tuning: RuntimeInputTuning {
            max_frames: 10,
            max_bytes: 32_000,
            max_buffered_media_duration: Duration::from_millis(200),
            target_merge_duration: Some(Duration::from_millis(20)),
            max_merge_duration: Some(Duration::from_millis(100)),
            max_in_flight: 1,
            deadline: Duration::from_millis(200),
            overflow_policy: policy,
        },
    }
}

#[test]
fn conservative_default_is_bounded_visible_and_valid() {
    let profile = RealtimeInputProfile::default();
    profile.validate().unwrap();
    assert_eq!(profile.tuning.max_frames, 8);
    assert_eq!(profile.tuning.max_in_flight, 1);
    assert_eq!(profile.contract.ordering, DeliveryOrdering::Strict);
    assert_eq!(
        profile.tuning.overflow_policy,
        AudioOverflowPolicy::PropagateBackpressure
    );
}

#[test]
fn profile_validation_rejects_zero_unbounded_and_contradictory_values() {
    let mut profile = audio_profile(AudioOverflowPolicy::AbortSession, false);
    profile.tuning.max_frames = 0;
    assert_eq!(profile.validate(), Err(RealtimeProfileError::ZeroCapacity));
    profile = audio_profile(AudioOverflowPolicy::AbortSession, false);
    profile.tuning.max_bytes = usize::MAX;
    assert_eq!(
        profile.validate(),
        Err(RealtimeProfileError::CapacityUnbounded)
    );
    profile = audio_profile(AudioOverflowPolicy::AbortSession, false);
    profile.contract.accepted_audio_duration = None;
    assert_eq!(
        profile.validate(),
        Err(RealtimeProfileError::AudioRangeRequired)
    );
    profile = audio_profile(AudioOverflowPolicy::AbortSession, false);
    profile.tuning.target_merge_duration = Some(Duration::from_millis(120));
    assert_eq!(
        profile.validate(),
        Err(RealtimeProfileError::InvalidMergeDuration)
    );
    profile = audio_profile(AudioOverflowPolicy::DropNewest, true);
    profile.contract.delivery_guarantee = DeliveryGuarantee::Lossless;
    assert_eq!(profile.validate(), Err(RealtimeProfileError::LosslessDrop));
    profile = audio_profile(AudioOverflowPolicy::DropSilenceFirst, false);
    profile.contract.permits_trusted_vad_silence_drop = false;
    assert_eq!(
        profile.validate(),
        Err(RealtimeProfileError::SilenceDropNotPermitted)
    );
    profile = audio_profile(AudioOverflowPolicy::PropagateBackpressure, false);
    assert_eq!(
        profile.validate(),
        Err(RealtimeProfileError::UnpausableBackpressureOnly)
    );
}

struct SequentialIds(u64);

impl FrameIdSource for SequentialIds {
    fn next_frame_id(&mut self) -> FrameId {
        let id = FrameId::new(format!("merged-{}", self.0)).unwrap();
        self.0 += 1;
        id
    }
}

fn audio_frame(index: u64, layout: AudioLayout) -> Frame {
    let samples = 960_usize;
    let bytes = match layout {
        AudioLayout::Interleaved => vec![u8::try_from(index + 1).unwrap(); samples * 2],
        AudioLayout::Planar => {
            let mut bytes = vec![u8::try_from(index * 2 + 1).unwrap(); samples];
            bytes.extend(vec![u8::try_from(index * 2 + 2).unwrap(); samples]);
            bytes
        }
    };
    let header = FrameHeader::new(
        FrameId::new(format!("audio-{index}")).unwrap(),
        Timestamp::from_nanos(i64::try_from(index).unwrap() * 20_000_000),
        ClockDomain::new(
            ClockDomainId::new("capture.audio").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(index),
        StreamId::new("microphone-1").unwrap(),
        TraceId::new("session-trace").unwrap(),
        FrameType::Audio,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    Frame::new(
        header,
        FramePayload::Audio(
            AudioData::new(
                FrameBuffer::from_vec(bytes),
                48_000,
                2,
                PcmSampleFormat::U8,
                layout,
                960,
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

fn merge_origin() -> TransformOrigin {
    TransformOrigin::new(Some(NodeId::new("asr").unwrap()), None).unwrap()
}

#[test]
fn merges_20ms_audio_to_80ms_with_exact_timing_bytes_and_lineage() {
    let frames = (0..5)
        .map(|index| audio_frame(index, AudioLayout::Interleaved))
        .collect::<Vec<_>>();
    let merged = merge_audio_prefix(
        &frames,
        Duration::from_millis(80),
        Duration::from_millis(100),
        &mut SequentialIds(1),
        merge_origin(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(merged.source_count(), 4);
    let frame = merged.frame();
    let audio = frame.as_audio().unwrap();
    assert_eq!(frame.header().frame_id().as_str(), "merged-1");
    assert_eq!(frame.header().timestamp(), Timestamp::from_nanos(0));
    assert_eq!(frame.header().sequence_id(), SequenceId::new(0));
    assert_eq!(audio.data().samples_per_channel(), 3_840);
    assert_eq!(audio.data().duration_ns(), 80_000_000);
    assert_eq!(audio.data().buffer().len(), 7_680);
    for (index, chunk) in audio.data().buffer().as_slice().chunks(1_920).enumerate() {
        assert!(chunk.iter().all(|byte| *byte == index as u8 + 1));
    }
    let entries = frame.header().lineage().iter().collect::<Vec<_>>();
    assert_eq!(entries.len(), 4);
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!(entry.parent_frame_id().as_str(), format!("audio-{index}"));
        assert_eq!(entry.reason(), "runtime_audio_merge");
        let range = entry.media_time_range().unwrap();
        assert_eq!(range.start().as_nanos(), index as i64 * 20_000_000);
        assert_eq!(range.end().as_nanos(), (index as i64 + 1) * 20_000_000);
        assert_eq!(range.clock_domain(), frame.header().clock_domain());
    }
}

#[test]
fn merges_20ms_planar_audio_to_100ms_by_channel_plane() {
    let frames = (0..5)
        .map(|index| audio_frame(index, AudioLayout::Planar))
        .collect::<Vec<_>>();
    let merged = merge_audio_prefix(
        &frames,
        Duration::from_millis(100),
        Duration::from_millis(100),
        &mut SequentialIds(7),
        merge_origin(),
    )
    .unwrap()
    .unwrap();
    let audio = merged.frame().as_audio().unwrap().data();
    assert_eq!(audio.duration_ns(), 100_000_000);
    assert_eq!(audio.samples_per_channel(), 4_800);
    for (channel, plane) in [audio.plane_bytes(0).unwrap(), audio.plane_bytes(1).unwrap()]
        .into_iter()
        .enumerate()
    {
        for (index, chunk) in plane.chunks(960).enumerate() {
            let expected = u8::try_from(index * 2 + channel + 1).unwrap();
            assert!(chunk.iter().all(|byte| *byte == expected));
        }
    }
}

fn low_rate_audio_frame(index: u64) -> Frame {
    let timestamp = match index {
        0 => 0,
        1 => 333_333_333,
        2 => 666_666_666,
        _ => panic!("test supports three sample boundaries"),
    };
    let header = FrameHeader::new(
        FrameId::new(format!("low-rate-{index}")).unwrap(),
        Timestamp::from_nanos(timestamp),
        ClockDomain::new(
            ClockDomainId::new("capture.low-rate").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(index),
        StreamId::new("low-rate-stream").unwrap(),
        TraceId::new("low-rate-trace").unwrap(),
        FrameType::Audio,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    Frame::new(
        header,
        FramePayload::Audio(
            AudioData::new(
                FrameBuffer::from_vec(vec![u8::try_from(index + 1).unwrap()]),
                3,
                1,
                PcmSampleFormat::U8,
                AudioLayout::Interleaved,
                1,
            )
            .unwrap(),
        ),
    )
    .unwrap()
}

#[test]
fn three_hz_merge_uses_total_samples_for_duration_boundaries_and_payload() {
    let frames = (0..3).map(low_rate_audio_frame).collect::<Vec<_>>();
    let merged = merge_audio_prefix(
        &frames,
        Duration::from_secs(1),
        Duration::from_secs(1),
        &mut SequentialIds(20),
        merge_origin(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(merged.source_count(), 3);
    let audio = merged.frame().as_audio().unwrap();
    assert_eq!(audio.data().duration_ns(), 1_000_000_000);
    assert_eq!(audio.data().buffer().as_slice(), &[1, 2, 3]);
    let ranges = merged
        .frame()
        .header()
        .lineage()
        .iter()
        .map(|entry| {
            let range = entry.media_time_range().unwrap();
            (range.start().as_nanos(), range.end().as_nanos())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ranges,
        [
            (0, 333_333_333),
            (333_333_333, 666_666_666),
            (666_666_666, 1_000_000_000),
        ]
    );

    assert!(merge_audio_prefix(
        &frames,
        Duration::from_nanos(999_999_999),
        Duration::from_nanos(999_999_999),
        &mut SequentialIds(21),
        merge_origin(),
    )
    .unwrap()
    .is_none());
}

#[test]
fn merge_leaves_mismatched_and_non_audio_prefixes_atomic() {
    let mut discontinuous = vec![
        audio_frame(0, AudioLayout::Interleaved),
        audio_frame(2, AudioLayout::Interleaved),
    ];
    assert!(merge_audio_prefix(
        &discontinuous,
        Duration::from_millis(40),
        Duration::from_millis(100),
        &mut SequentialIds(1),
        merge_origin(),
    )
    .unwrap()
    .is_none());
    let text_header = FrameHeader::new(
        FrameId::new("text").unwrap(),
        Timestamp::from_nanos(0),
        discontinuous[0].header().clock_domain().clone(),
        SequenceId::new(0),
        StreamId::new("microphone-1").unwrap(),
        TraceId::new("session-trace").unwrap(),
        FrameType::Text,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    discontinuous[0] = Frame::new(
        text_header,
        FramePayload::Text(muxiva_types::TextData::new("not audio")),
    )
    .unwrap();
    assert!(merge_audio_prefix(
        &discontinuous,
        Duration::from_millis(40),
        Duration::from_millis(100),
        &mut SequentialIds(1),
        merge_origin(),
    )
    .unwrap()
    .is_none());
}

#[derive(Default)]
struct FakeClock(AtomicU64);

impl FakeClock {
    fn advance(&self, duration: Duration) {
        self.0.fetch_add(
            u64::try_from(duration.as_nanos()).unwrap(),
            Ordering::SeqCst,
        );
    }
}

impl FlowClock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.0.load(Ordering::SeqCst))
    }
}

fn controller(
    port: &str,
    profile: RealtimeInputProfile,
    clock: Arc<FakeClock>,
) -> AdaptiveFlowController {
    AdaptiveFlowController::new(
        InputPortKey {
            node_id: NodeId::new("asr").unwrap(),
            port: PortName::new(port).unwrap(),
        },
        profile,
        clock,
    )
    .unwrap()
}

#[test]
fn slow_asr_predicts_pressure_then_critical_before_queue_limit() {
    let clock = Arc::new(FakeClock::default());
    let mut controller = controller(
        "audio",
        audio_profile(AudioOverflowPolicy::PropagateBackpressure, true),
        clock.clone(),
    );
    let audio = FrameMeasurement::new(1_920, Duration::from_millis(20));

    controller.record_enqueue(audio).unwrap();
    let (work, _) = controller.record_admission(audio).unwrap();
    for _ in 0..4 {
        clock.advance(Duration::from_millis(20));
        controller.record_enqueue(audio).unwrap();
    }
    let pressure = controller.record_completion(work).unwrap();
    assert_eq!(pressure.snapshot.state, FlowState::Pressure);
    assert!(pressure.snapshot.predicted_time_to_saturation.is_some());
    assert!(pressure.snapshot.service_time_ema >= Duration::from_millis(80));
    assert_eq!(
        controller.signal_observations().collect::<Vec<_>>(),
        [FlowSignalObservation::Pressure]
    );

    let mut critical = pressure;
    while critical.snapshot.state != FlowState::Critical {
        clock.advance(Duration::from_millis(20));
        critical = controller.record_enqueue(audio).unwrap();
    }
    assert!(critical.snapshot.queued_frames < 10);
    assert_eq!(
        critical.snapshot.input_media_duration_total,
        Duration::from_millis((critical.snapshot.queued_frames as u64 + 1) * 20)
    );
    assert_eq!(
        critical.snapshot.completed_media_duration_total,
        Duration::from_millis(20)
    );
    assert!(critical.actions.contains(&FlowAction::ApplyOverflow(
        AudioOverflowPolicy::PropagateBackpressure
    )));
    assert_eq!(
        controller.signal_observations().collect::<Vec<_>>(),
        [FlowSignalObservation::Pressure]
    );
}

#[test]
fn completion_token_is_tied_to_its_specific_controller_admission() {
    let clock = Arc::new(FakeClock::default());
    let mut first = controller(
        "first",
        audio_profile(AudioOverflowPolicy::PropagateBackpressure, true),
        clock.clone(),
    );
    let mut second = controller(
        "second",
        audio_profile(AudioOverflowPolicy::PropagateBackpressure, true),
        clock,
    );
    let measurement = FrameMeasurement::new(1_920, Duration::from_millis(20));
    first.record_enqueue(measurement).unwrap();
    second.record_enqueue(measurement).unwrap();
    let (first_work, _) = first.record_admission(measurement).unwrap();
    let (second_work, _) = second.record_admission(measurement).unwrap();

    assert_eq!(
        first.record_completion(second_work),
        Err(muxiva_core::FlowError::CompletionWithoutAdmission)
    );
    assert_eq!(first.snapshot().in_flight, 1);
    first.record_completion(first_work).unwrap();
    assert_eq!(first.snapshot().in_flight, 0);
    assert_eq!(second.snapshot().in_flight, 1);
}

#[test]
fn frame_byte_and_media_limits_bind_independently_and_resume_is_hysteretic() {
    let measurement = FrameMeasurement::new(120, Duration::from_millis(20));
    for axis in 0..3 {
        let clock = Arc::new(FakeClock::default());
        let mut profile = audio_profile(AudioOverflowPolicy::DropOldest, true);
        profile.tuning.max_frames = if axis == 0 { 2 } else { 10 };
        profile.tuning.max_bytes = if axis == 1 { 200 } else { 10_000 };
        profile.tuning.max_buffered_media_duration = if axis == 2 {
            Duration::from_millis(30)
        } else {
            Duration::from_millis(200)
        };
        profile.tuning.max_merge_duration = Some(
            profile
                .tuning
                .max_buffered_media_duration
                .min(Duration::from_millis(100)),
        );
        let mut flow = controller("axis", profile, clock.clone());
        flow.record_enqueue(measurement).unwrap();
        clock.advance(Duration::from_millis(20));
        let update = flow.record_enqueue(measurement).unwrap();
        assert_eq!(update.snapshot.state, FlowState::Critical);
        match axis {
            0 => assert_eq!(update.snapshot.queued_frames, 2),
            1 => assert_eq!(update.snapshot.queued_bytes, 240),
            _ => assert_eq!(
                update.snapshot.queued_media_duration,
                Duration::from_millis(40)
            ),
        }
    }

    let clock = Arc::new(FakeClock::default());
    let mut flow = controller(
        "resume",
        audio_profile(AudioOverflowPolicy::DropOldest, true),
        clock.clone(),
    );
    let audio = FrameMeasurement::new(1_920, Duration::from_millis(20));
    for _ in 0..8 {
        flow.record_enqueue(audio).unwrap();
    }
    assert_ne!(flow.snapshot().state, FlowState::Normal);
    let mut resumed_after = None;
    let mut resumed = None;
    for dropped in 1..=8 {
        let update = flow
            .record_drop(audio, muxiva_core::FlowDropReason::PressureShedding)
            .unwrap();
        if update.actions.contains(&FlowAction::EmitResume) {
            resumed_after = Some(dropped);
            resumed = Some(update);
        }
    }
    assert_eq!(resumed_after, Some(6));
    let resumed = resumed.unwrap();
    assert_eq!(resumed.snapshot.state, FlowState::Normal);
    assert!(resumed.actions.contains(&FlowAction::EmitResume));
    let final_snapshot = flow.snapshot();
    assert_eq!(final_snapshot.dropped_frames_total, 8);
    assert_eq!(final_snapshot.dropped_bytes_total, 15_360);
    assert_eq!(
        final_snapshot.dropped_media_duration_total,
        Duration::from_millis(160)
    );
}

#[test]
fn overflow_decisions_cover_every_policy_and_trusted_vad_guard() {
    let clock = Arc::new(FakeClock::default());
    let cases = [
        (
            AudioOverflowPolicy::PropagateBackpressure,
            true,
            None,
            OverflowDecision::PropagateBackpressure,
        ),
        (
            AudioOverflowPolicy::DropOldest,
            true,
            None,
            OverflowDecision::DropOldest,
        ),
        (
            AudioOverflowPolicy::DropNewest,
            true,
            None,
            OverflowDecision::DropNewest,
        ),
        (
            AudioOverflowPolicy::DropSilenceFirst,
            false,
            Some(TrustedVadDecision::Silence),
            OverflowDecision::DropTrustedSilence,
        ),
        (
            AudioOverflowPolicy::AbortSession,
            false,
            None,
            OverflowDecision::AbortSession,
        ),
    ];
    for (policy, pausable, vad, expected) in cases {
        let profile = audio_profile(policy, pausable);
        let controller = controller("audio", profile, clock.clone());
        assert_eq!(controller.decide_overflow(vad), expected);
    }

    let silence = controller(
        "vad",
        audio_profile(AudioOverflowPolicy::DropSilenceFirst, false),
        clock,
    );
    assert_eq!(
        silence.decide_overflow(None),
        OverflowDecision::AbortSession
    );
    assert_eq!(
        silence.decide_overflow(Some(TrustedVadDecision::Speech)),
        OverflowDecision::AbortSession
    );
}

#[test]
fn lossless_profiles_never_select_silent_loss_and_ports_are_independent() {
    let clock = Arc::new(FakeClock::default());
    let lossless = audio_profile(AudioOverflowPolicy::PropagateBackpressure, true);
    assert!(!overflow_may_drop(&lossless));
    let abort = audio_profile(AudioOverflowPolicy::AbortSession, false);
    assert!(!overflow_may_drop(&abort));

    let mut left = controller("left", lossless.clone(), clock.clone());
    let right = controller("right", lossless, clock);
    let audio = FrameMeasurement::new(1_920, Duration::from_millis(20));
    left.record_enqueue(audio).unwrap();
    assert_eq!(
        left.record_drop(audio, muxiva_core::FlowDropReason::PressureShedding),
        Err(muxiva_core::FlowError::LosslessDropForbidden)
    );
    for _ in 1..8 {
        left.record_enqueue(audio).unwrap();
    }
    assert_ne!(left.snapshot().state, FlowState::Normal);
    assert_eq!(right.snapshot().state, FlowState::Normal);
    assert_eq!(right.snapshot().queued_frames, 0);
    assert_eq!(left.snapshot().key.port.as_str(), "left");
    assert_eq!(right.snapshot().key.port.as_str(), "right");
}

#[test]
fn admission_slots_are_held_until_sync_or_async_completion_and_return_once() {
    let slots = AdmissionSlots::new(2).unwrap();
    let first = slots.try_acquire().unwrap().unwrap();
    let second = slots.try_acquire().unwrap().unwrap();
    assert!(slots.try_acquire().unwrap().is_none());
    assert_eq!(slots.snapshot().in_flight, 2);
    first.release();
    assert_eq!(slots.snapshot().in_flight, 1);
    drop(second);
    assert_eq!(slots.snapshot().in_flight, 0);

    let value = slots.with_slot(|| 42).unwrap();
    assert_eq!(value, 42);
    assert_eq!(slots.snapshot().in_flight, 0);
    slots.close();
    assert!(slots.try_acquire().is_err());
}
