use std::cmp::Ordering;

use muxiva_types::{
    AudioData, AudioLayout, ByteData, ClockDomain, ClockDomainId, ClockKind, EventData, Extensions,
    FiniteF64, Frame, FrameBuffer, FrameHeader, FrameId, FramePayload, FrameType, Lineage,
    MediaType, Metadata, NamespacedName, NodeId, PcmSampleFormat, PixelFormat, SchemaVersion,
    SequenceId, SignalData, StreamId, TextData, Timestamp, TraceId, Value, ValueMap, VideoData,
    VideoLayout,
};

fn media_domain(id: &str) -> ClockDomain {
    ClockDomain::new(ClockDomainId::new(id).unwrap(), ClockKind::MediaRelative)
}

fn header_in(clock_domain: ClockDomain, timestamp: Timestamp) -> FrameHeader {
    header_for(FrameType::Audio, clock_domain, timestamp)
}

fn header_for(
    frame_type: FrameType,
    clock_domain: ClockDomain,
    timestamp: Timestamp,
) -> FrameHeader {
    FrameHeader::new(
        FrameId::new("frame-1").unwrap(),
        timestamp,
        clock_domain,
        SequenceId::new(1),
        StreamId::new("stream-1").unwrap(),
        TraceId::new("trace-1").unwrap(),
        frame_type,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap()
}

#[test]
fn messages_validate_owned_text_and_media_types() {
    assert_eq!(
        TextData::from_utf8(FrameBuffer::from_vec(vec![0xff]))
            .unwrap_err()
            .code(),
        "MUXIVA-FRM-TEXT-UTF8"
    );
    let text = TextData::from_utf8(FrameBuffer::from_vec("hello".as_bytes().to_vec())).unwrap();
    assert_eq!(text.as_str(), "hello");
    assert_eq!(TextData::new(String::from("owned")).as_str(), "owned");

    assert_eq!(MediaType::new("audio/pcm").unwrap().as_str(), "audio/pcm");
    for invalid in [
        "",
        "audio",
        "/pcm",
        "audio/",
        "audio/pcm/extra",
        "audio /pcm",
        "audio/p\u{00e4}cm",
        &"a".repeat(128),
    ] {
        assert_eq!(
            MediaType::new(invalid).unwrap_err().code(),
            "MUXIVA-FRM-MEDIA-TYPE"
        );
    }

    let bytes = ByteData::new(FrameBuffer::from_vec(Vec::new()), None);
    assert!(bytes.buffer().is_empty());
    assert!(bytes.media_type().is_none());
}

#[test]
fn messages_hold_namespaced_signal_and_event_values_without_timestamps() {
    let all_values = Value::List(
        vec![
            Value::Null,
            Value::Bool(true),
            Value::Integer(-7),
            Value::Float(FiniteF64::new(1.5).unwrap()),
            Value::String("text".into()),
            Value::Bytes(FrameBuffer::from_vec(vec![1, 2])),
            Value::List(vec![Value::Bool(false)].into_boxed_slice()),
            Value::Map(ValueMap::try_from_iter([("key", Value::Null)]).unwrap()),
        ]
        .into_boxed_slice(),
    );
    let signal = SignalData::new(
        NamespacedName::new("muxiva.signal.ready").unwrap(),
        SchemaVersion::new(1).unwrap(),
        NodeId::new("source").unwrap(),
        all_values.clone(),
    );
    assert_eq!(signal.name().as_str(), "muxiva.signal.ready");
    assert_eq!(signal.schema_version().get(), 1);
    assert_eq!(signal.source().as_str(), "source");
    assert_eq!(signal.payload(), &all_values);
    let signal_timestamp = Timestamp::from_nanos(41);
    let signal_frame = Frame::new(
        header_for(FrameType::Signal, media_domain("signals"), signal_timestamp),
        FramePayload::Signal(signal),
    )
    .unwrap();
    assert_eq!(signal_frame.header().timestamp(), signal_timestamp);

    let event = EventData::new(
        NamespacedName::new("muxiva.event.ready").unwrap(),
        SchemaVersion::new(2).unwrap(),
        NodeId::new("publisher").unwrap(),
        all_values,
    );
    assert_eq!(event.topic().as_str(), "muxiva.event.ready");
    assert_eq!(event.schema_version().get(), 2);
    assert_eq!(event.source().as_str(), "publisher");

    let timestamp = Timestamp::from_nanos(42);
    let frame = Frame::new(
        header_for(FrameType::Event, media_domain("events"), timestamp),
        FramePayload::Event(event),
    )
    .unwrap();
    assert_eq!(frame.header().timestamp(), timestamp);
}

#[test]
fn frame_variants_dispatch_and_enforce_the_header_type() {
    let payloads = [
        FramePayload::Audio(
            AudioData::new(
                FrameBuffer::from_vec(vec![0]),
                48_000,
                1,
                PcmSampleFormat::U8,
                AudioLayout::Interleaved,
                1,
            )
            .unwrap(),
        ),
        FramePayload::Video(VideoData::rgba8(FrameBuffer::from_vec(vec![0; 4]), 1, 1, 4).unwrap()),
        FramePayload::Text(TextData::new("hello")),
        FramePayload::Byte(ByteData::new(FrameBuffer::from_vec(Vec::new()), None)),
        FramePayload::Signal(SignalData::new(
            NamespacedName::new("muxiva.signal.ready").unwrap(),
            SchemaVersion::new(1).unwrap(),
            NodeId::new("source").unwrap(),
            Value::Null,
        )),
        FramePayload::Event(EventData::new(
            NamespacedName::new("muxiva.event.ready").unwrap(),
            SchemaVersion::new(1).unwrap(),
            NodeId::new("source").unwrap(),
            Value::Null,
        )),
    ];

    for payload in payloads {
        let expected = payload.frame_type();
        let frame = Frame::new(
            header_for(expected, media_domain("frames"), Timestamp::from_nanos(7)),
            payload,
        )
        .unwrap();
        assert_eq!(frame.frame_type(), expected);
        assert_eq!(frame.header().frame_type(), expected);
        assert_eq!(
            [
                frame.as_audio().is_some(),
                frame.as_video().is_some(),
                frame.as_text().is_some(),
                frame.as_byte().is_some(),
                frame.as_signal().is_some(),
                frame.as_event().is_some(),
            ]
            .into_iter()
            .filter(|present| *present)
            .count(),
            1
        );
        frame.ensure_type(expected).unwrap();
        assert_eq!(
            frame
                .ensure_type(different_type(expected))
                .unwrap_err()
                .code(),
            "MUXIVA-FRM-TYPE-MISMATCH"
        );
    }

    let error = Frame::new(
        header_for(
            FrameType::Audio,
            media_domain("mismatch"),
            Timestamp::from_nanos(0),
        ),
        FramePayload::Text(TextData::new("hello")),
    )
    .unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-TYPE-MISMATCH");

    let text = Frame::new(
        header_for(
            FrameType::Text,
            media_domain("debug"),
            Timestamp::from_nanos(0),
        ),
        FramePayload::Text(TextData::new("private payload contents")),
    )
    .unwrap();
    let rendered = format!("{text:?}");
    assert!(rendered.contains("payload_byte_len: Some(24)"));
    assert!(!rendered.contains("private payload contents"));
}

fn different_type(frame_type: FrameType) -> FrameType {
    match frame_type {
        FrameType::Audio => FrameType::Video,
        _ => FrameType::Audio,
    }
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
    assert_eq!(error.code(), "MUXIVA-FRM-CLOCK-DOMAIN");
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
    for (rate, code) in [
        (0, "MUXIVA-FRM-AUDIO-RATE"),
        (768_001, "MUXIVA-FRM-AUDIO-RATE"),
    ] {
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
        assert_eq!(error.code(), "MUXIVA-FRM-AUDIO-CHANNELS");
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
    assert_eq!(error.code(), "MUXIVA-FRM-AUDIO-SAMPLES");
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
        assert_eq!(error.code(), "MUXIVA-FRM-AUDIO-LENGTH");
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
    assert_eq!(error.code(), "MUXIVA-FRM-ARITHMETIC");
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
        "MUXIVA-FRM-AUDIO-PLANE"
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
        "MUXIVA-FRM-AUDIO-PLANE"
    );
    assert_eq!(
        planar.plane_bytes(u16::MAX).unwrap_err().code(),
        "MUXIVA-FRM-AUDIO-PLANE"
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
        assert_eq!(error.code(), "MUXIVA-FRM-VIDEO-DIMENSIONS");
    }

    for (width, height) in [(0, 2), (2, 0), (3, 2), (2, 3)] {
        let error = VideoData::yuv420p(FrameBuffer::from_vec(Vec::new()), width, height, 4, 2, 2)
            .unwrap_err();
        assert_eq!(error.code(), "MUXIVA-FRM-VIDEO-DIMENSIONS");
    }
}

#[test]
fn video_rejects_short_strides() {
    let rgba_error = VideoData::rgba8(FrameBuffer::from_vec(Vec::new()), 2, 2, 7).unwrap_err();
    assert_eq!(rgba_error.code(), "MUXIVA-FRM-VIDEO-STRIDE");

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
        assert_eq!(error.code(), "MUXIVA-FRM-VIDEO-STRIDE");
    }
}

#[test]
fn video_requires_exact_payload_length() {
    for length in [15, 17] {
        let error = VideoData::rgba8(FrameBuffer::from_vec(vec![0; length]), 2, 2, 8).unwrap_err();
        assert_eq!(error.code(), "MUXIVA-FRM-VIDEO-LENGTH");
    }

    for length in [11, 13] {
        let error =
            VideoData::yuv420p(FrameBuffer::from_vec(vec![0; length]), 4, 2, 4, 2, 2).unwrap_err();
        assert_eq!(error.code(), "MUXIVA-FRM-VIDEO-LENGTH");
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
    assert_eq!(error.code(), "MUXIVA-FRM-ARITHMETIC");

    let error = VideoData::yuv420p(
        FrameBuffer::from_vec(Vec::new()),
        u32::MAX - 1,
        u32::MAX - 1,
        usize::MAX,
        usize::MAX,
        usize::MAX,
    )
    .unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-ARITHMETIC");
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
        "MUXIVA-FRM-VIDEO-PLANE"
    );
}
