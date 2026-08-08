#![forbid(unsafe_code)]

use muxiva_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, EdgeId, Extension,
    ExtensionProducer, ExtensionVisibility, Extensions, Frame, FrameBuffer, FrameDerivation,
    FrameHeader, FrameId, FramePayload, FrameType, Lineage, Metadata, NamespacedName, NodeId,
    PcmSampleFormat, SchemaVersion, SequenceId, StreamId, Timestamp, TraceId, TransformOrigin,
    Value,
};

fn main() {
    let extensions = Extensions::try_from_iter([
        Extension::new(
            NamespacedName::new("com.example.future").expect("valid extension name"),
            SchemaVersion::new(1).expect("valid schema version"),
            ExtensionProducer::Core,
            ExtensionVisibility::Public,
            Value::String("preserved by default derivation".into()),
        ),
        Extension::new(
            NamespacedName::new("com.example.private_context").expect("valid extension name"),
            SchemaVersion::new(1).expect("valid schema version"),
            ExtensionProducer::Core,
            ExtensionVisibility::Private,
            Value::String("private capture context".into()),
        ),
    ])
    .expect("unique extensions");
    let parent_buffer = FrameBuffer::from_vec(vec![0; 960]);
    let parent = Frame::new(
        FrameHeader::new(
            FrameId::new("frame-1").expect("valid frame ID"),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("capture.audio").expect("valid clock domain ID"),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(1),
            StreamId::new("capture-stream").expect("valid stream ID"),
            TraceId::new("capture-trace").expect("valid trace ID"),
            FrameType::Audio,
            Metadata::empty(),
            extensions,
            Lineage::empty(),
        )
        .expect("valid source header"),
        FramePayload::Audio(
            AudioData::new(
                parent_buffer.clone(),
                48_000,
                1,
                PcmSampleFormat::I16Le,
                AudioLayout::Interleaved,
                480,
            )
            .expect("valid source audio"),
        ),
    )
    .expect("matching source frame types");

    let replacement_buffer = FrameBuffer::from_vec(vec![0; 960]);
    let child = parent
        .derive(
            FrameDerivation::new(
                FrameId::new("frame-2").expect("valid frame ID"),
                Timestamp::from_nanos(10_000_000),
                SequenceId::new(2),
                TransformOrigin::new(
                    Some(NodeId::new("normalize").expect("valid node ID")),
                    Some(EdgeId::new("capture-to-normalize").expect("valid edge ID")),
                )
                .expect("valid transform origin"),
                "normalize-volume",
            )
            .expect("valid frame derivation")
            .with_payload(FramePayload::Audio(
                AudioData::new(
                    replacement_buffer.clone(),
                    48_000,
                    1,
                    PcmSampleFormat::I16Le,
                    AudioLayout::Interleaved,
                    480,
                )
                .expect("valid derived audio"),
            )),
        )
        .expect("valid child frame");

    assert_eq!(parent.header().frame_id().as_str(), "frame-1");
    assert!(parent.header().lineage().is_empty());
    assert_eq!(
        parent.as_audio().expect("audio parent").data().buffer(),
        &parent_buffer
    );
    assert_eq!(child.header().frame_id().as_str(), "frame-2");
    assert_eq!(child.header().lineage().len(), 1);
    assert!(child
        .header()
        .extensions()
        .iter()
        .any(|extension| extension.key().as_str() == "com.example.future"));
    assert_eq!(
        child.as_audio().expect("audio child").data().buffer(),
        &replacement_buffer
    );
    assert_ne!(
        parent
            .as_audio()
            .expect("audio parent")
            .data()
            .buffer()
            .as_slice()
            .as_ptr(),
        child
            .as_audio()
            .expect("audio child")
            .data()
            .buffer()
            .as_slice()
            .as_ptr()
    );

    let log_safe = child.log_safe_view();
    println!(
        "Muxiva derived frame: {} {:?} lineage={}",
        log_safe.frame_id(),
        log_safe.frame_type(),
        log_safe.lineage_count()
    );
}
