use std::cmp::Ordering;

use voxa_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, Extensions, FrameBuffer,
    FrameHeader, FrameId, FrameType, Lineage, Metadata, PcmSampleFormat, PixelFormat, SequenceId,
    StreamId, Timestamp, TraceId, VideoData, VideoLayout,
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

#[test]
fn video_rgba8_layout_reports_exact_plane_geometry_and_bytes() {
    let video = VideoData::rgba8(FrameBuffer::from_vec((0..16).collect()), 2, 2, 8).unwrap();

    assert_eq!(video.width(), 2);
    assert_eq!(video.height(), 2);
    assert_eq!(video.pixel_format(), PixelFormat::Rgba8);
    assert_eq!(video.buffer().len(), 16);
    let VideoLayout::Rgba8 { plane } = video.layout() else {
        panic!("expected RGBA8 layout");
    };
    assert_eq!(plane.offset(), 0);
    assert_eq!(plane.stride(), 8);
    assert_eq!(plane.row_bytes(), 8);
    assert_eq!(plane.rows(), 2);
    assert_eq!(
        video.plane_bytes(plane).unwrap(),
        &(0..16).collect::<Vec<_>>()
    );
}

#[test]
fn video_yuv420p_layout_reports_tightly_sequenced_planes() {
    let video =
        VideoData::yuv420p(FrameBuffer::from_vec((0..12).collect()), 4, 2, 4, 2, 2).unwrap();

    assert_eq!(video.width(), 4);
    assert_eq!(video.height(), 2);
    assert_eq!(video.pixel_format(), PixelFormat::Yuv420p);
    assert_eq!(video.buffer().len(), 12);
    let VideoLayout::Yuv420p { y, u, v } = video.layout() else {
        panic!("expected YUV420P layout");
    };
    assert_eq!(
        (y.offset(), y.stride(), y.row_bytes(), y.rows()),
        (0, 4, 4, 2)
    );
    assert_eq!(
        (u.offset(), u.stride(), u.row_bytes(), u.rows()),
        (8, 2, 2, 1)
    );
    assert_eq!(
        (v.offset(), v.stride(), v.row_bytes(), v.rows()),
        (10, 2, 2, 1)
    );
    assert_eq!(video.plane_bytes(y).unwrap(), &[0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(video.plane_bytes(u).unwrap(), &[8, 9]);
    assert_eq!(video.plane_bytes(v).unwrap(), &[10, 11]);
}

#[test]
fn video_rejects_invalid_dimensions() {
    for (width, height) in [(0, 1), (1, 0)] {
        let error =
            VideoData::rgba8(FrameBuffer::from_vec(Vec::new()), width, height, 4).unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-VIDEO-DIMENSIONS");
    }

    for (width, height) in [(0, 2), (2, 0), (3, 2), (2, 3)] {
        let error = VideoData::yuv420p(FrameBuffer::from_vec(Vec::new()), width, height, 4, 2, 2)
            .unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-VIDEO-DIMENSIONS");
    }
}

#[test]
fn video_rejects_short_strides() {
    let rgba_error = VideoData::rgba8(FrameBuffer::from_vec(Vec::new()), 2, 2, 7).unwrap_err();
    assert_eq!(rgba_error.code(), "VOXA-FRM-VIDEO-STRIDE");

    for strides in [(3, 2, 2), (4, 1, 2), (4, 2, 1)] {
        let error = VideoData::yuv420p(
            FrameBuffer::from_vec(Vec::new()),
            4,
            2,
            strides.0,
            strides.1,
            strides.2,
        )
        .unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-VIDEO-STRIDE");
    }
}

#[test]
fn video_requires_exact_payload_length() {
    for length in [15, 17] {
        let error = VideoData::rgba8(FrameBuffer::from_vec(vec![0; length]), 2, 2, 8).unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-VIDEO-LENGTH");
    }

    for length in [11, 13] {
        let error =
            VideoData::yuv420p(FrameBuffer::from_vec(vec![0; length]), 4, 2, 4, 2, 2).unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-VIDEO-LENGTH");
    }
}

#[test]
fn video_rejects_size_arithmetic_overflow_before_accessing_payload() {
    let error = VideoData::rgba8(
        FrameBuffer::from_vec(Vec::new()),
        u32::MAX,
        u32::MAX,
        usize::MAX,
    )
    .unwrap_err();
    assert_eq!(error.code(), "VOXA-FRM-ARITHMETIC");

    let error = VideoData::yuv420p(
        FrameBuffer::from_vec(Vec::new()),
        u32::MAX - 1,
        u32::MAX - 1,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    )
    .unwrap_err();
    assert_eq!(error.code(), "VOXA-FRM-ARITHMETIC");
}

#[test]
fn video_plane_bytes_require_descriptor_identity() {
    let first = VideoData::rgba8(FrameBuffer::from_vec((0..16).collect()), 2, 2, 8).unwrap();
    let second = VideoData::rgba8(FrameBuffer::from_vec((16..32).collect()), 2, 2, 8).unwrap();
    let VideoLayout::Rgba8 { plane: first_plane } = first.layout() else {
        panic!("expected RGBA8 layout");
    };
    let VideoLayout::Rgba8 {
        plane: second_plane,
    } = second.layout()
    else {
        panic!("expected RGBA8 layout");
    };

    assert_eq!(
        first.plane_bytes(first_plane).unwrap(),
        &(0..16).collect::<Vec<_>>()
    );
    assert_eq!(
        second.plane_bytes(second_plane).unwrap(),
        &(16..32).collect::<Vec<_>>()
    );
    assert_eq!(
        second.plane_bytes(first_plane).unwrap_err().code(),
        "VOXA-FRM-VIDEO-PLANE"
    );
}
