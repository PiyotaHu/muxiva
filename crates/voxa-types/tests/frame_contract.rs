use std::cmp::Ordering;

use voxa_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, Extensions, FrameBuffer,
    FrameHeader, FrameId, FrameType, Lineage, Metadata, PcmSampleFormat, SequenceId, StreamId,
    Timestamp, TraceId,
};

fn media_domain(id: &str) -> ClockDomain {
    ClockDomain::new(ClockDomainId::new(id).unwrap(), ClockKind::MediaRelative)
}

fn header_in(clock_domain: ClockDomain, timestamp: Timestamp) -> FrameHeader {
    FrameHeader::new(
        FrameId::new("frame-1").unwrap(),
        timestamp,
        clock_domain,
        SequenceId::new(1),
        StreamId::new("stream-1").unwrap(),
        TraceId::new("trace-1").unwrap(),
        FrameType::Audio,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap()
}

#[test]
fn header_compares_timestamps_only_inside_one_clock_domain() {
    let domain = ClockDomain::new(
        ClockDomainId::new("capture.audio").unwrap(),
        ClockKind::MediaRelative,
    );
    let earlier = header_in(domain.clone(), Timestamp::from_nanos(-1));
    let later = header_in(domain, Timestamp::from_nanos(2));
    assert_eq!(earlier.compare_timestamp(&later).unwrap(), Ordering::Less);
    assert_eq!(
        later.compare_timestamp(&earlier).unwrap(),
        Ordering::Greater
    );
    assert_eq!(
        earlier.compare_timestamp(&earlier).unwrap(),
        Ordering::Equal
    );
}

#[test]
fn header_rejects_same_kind_with_different_clock_ids() {
    let left = header_in(media_domain("capture.left"), Timestamp::from_nanos(1));
    let right = header_in(media_domain("capture.right"), Timestamp::from_nanos(2));
    let error = left.compare_timestamp(&right).unwrap_err();
    assert_eq!(error.code(), "VOXA-FRM-CLOCK-DOMAIN");
}

#[test]
fn constructs_interleaved_pcm_and_duration() {
    let audio = AudioData::new(
        FrameBuffer::from_vec(vec![0; 1_920]),
        48_000,
        2,
        PcmSampleFormat::I16Le,
        AudioLayout::Interleaved,
        480,
    )
    .unwrap();
    assert_eq!(audio.duration_ns(), 10_000_000);
    assert_eq!(audio.plane_bytes(0).unwrap().len(), 1_920);
}

#[test]
fn audio_rejects_invalid_scalar_limits() {
    for (rate, code) in [(0, "VOXA-FRM-AUDIO-RATE"), (768_001, "VOXA-FRM-AUDIO-RATE")] {
        let error = AudioData::new(
            FrameBuffer::from_vec(vec![0; 2]),
            rate,
            1,
            PcmSampleFormat::I16Le,
            AudioLayout::Interleaved,
            1,
        )
        .unwrap_err();
        assert_eq!(error.code(), code);
    }

    for channels in [0, 1_025] {
        let error = AudioData::new(
            FrameBuffer::from_vec(vec![0; 2]),
            48_000,
            channels,
            PcmSampleFormat::I16Le,
            AudioLayout::Interleaved,
            1,
        )
        .unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-AUDIO-CHANNELS");
    }

    let error = AudioData::new(
        FrameBuffer::from_vec(Vec::new()),
        48_000,
        1,
        PcmSampleFormat::I16Le,
        AudioLayout::Interleaved,
        0,
    )
    .unwrap_err();
    assert_eq!(error.code(), "VOXA-FRM-AUDIO-SAMPLES");
}

#[test]
fn audio_requires_exact_payload_length() {
    for length in [3, 5] {
        let error = AudioData::new(
            FrameBuffer::from_vec(vec![0; length]),
            48_000,
            2,
            PcmSampleFormat::I16Le,
            AudioLayout::Interleaved,
            1,
        )
        .unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-AUDIO-LENGTH");
    }
}

#[test]
fn audio_rejects_size_arithmetic_overflow_before_accessing_payload() {
    let error = AudioData::new(
        FrameBuffer::from_vec(Vec::new()),
        48_000,
        2,
        PcmSampleFormat::F64Le,
        AudioLayout::Planar,
        u64::MAX,
    )
    .unwrap_err();
    assert_eq!(error.code(), "VOXA-FRM-ARITHMETIC");
}

#[test]
fn audio_plane_bytes_follow_layout_semantics() {
    let interleaved = AudioData::new(
        FrameBuffer::from_vec(vec![0, 1, 2, 3, 4, 5]),
        48_000,
        3,
        PcmSampleFormat::I16Le,
        AudioLayout::Interleaved,
        1,
    )
    .unwrap();
    assert_eq!(interleaved.plane_bytes(0).unwrap(), &[0, 1, 2, 3, 4, 5]);
    assert_eq!(
        interleaved.plane_bytes(1).unwrap_err().code(),
        "VOXA-FRM-AUDIO-PLANE"
    );

    let planar = AudioData::new(
        FrameBuffer::from_vec((0..12).collect()),
        48_000,
        3,
        PcmSampleFormat::I16Le,
        AudioLayout::Planar,
        2,
    )
    .unwrap();
    assert_eq!(planar.plane_bytes(0).unwrap(), &[0, 1, 2, 3]);
    assert_eq!(planar.plane_bytes(1).unwrap(), &[4, 5, 6, 7]);
    assert_eq!(planar.plane_bytes(2).unwrap(), &[8, 9, 10, 11]);
    assert_eq!(
        planar.plane_bytes(3).unwrap_err().code(),
        "VOXA-FRM-AUDIO-PLANE"
    );
    assert_eq!(
        planar.plane_bytes(u16::MAX).unwrap_err().code(),
        "VOXA-FRM-AUDIO-PLANE"
    );
}

#[test]
fn pcm_formats_report_exact_sample_widths() {
    assert_eq!(PcmSampleFormat::U8.bytes_per_sample(), 1);
    assert_eq!(PcmSampleFormat::I16Le.bytes_per_sample(), 2);
    assert_eq!(PcmSampleFormat::I24Le.bytes_per_sample(), 3);
    assert_eq!(PcmSampleFormat::I32Le.bytes_per_sample(), 4);
    assert_eq!(PcmSampleFormat::F32Le.bytes_per_sample(), 4);
    assert_eq!(PcmSampleFormat::F64Le.bytes_per_sample(), 8);
}
