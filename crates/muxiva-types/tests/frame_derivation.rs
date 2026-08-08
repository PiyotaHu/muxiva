use muxiva_types::{
    ByteData, ClockDomain, ClockDomainId, ClockKind, EdgeId, Extension, ExtensionProducer,
    ExtensionVisibility, Extensions, Frame, FrameBuffer, FrameDerivation, FrameHeader, FrameId,
    FramePayload, FrameType, Lineage, Metadata, NamespacedName, NodeId, PublicFrameHeaderView,
    SchemaVersion, SequenceId, StreamId, TextData, Timestamp, TraceId, TransformOrigin, Value,
};

fn extension(key: &str, visibility: ExtensionVisibility, value: &str) -> Extension {
    Extension::new(
        NamespacedName::new(key).unwrap(),
        SchemaVersion::new(7).unwrap(),
        ExtensionProducer::Node(NodeId::new("extension-producer").unwrap()),
        visibility,
        Value::String(value.into()),
    )
}

fn frame_with_public_and_private_extensions() -> Frame {
    let metadata = Metadata::try_from_iter([(
        "capture-device",
        Value::String("private-device-name".into()),
    )])
    .unwrap();
    let extensions = Extensions::try_from_iter([
        extension(
            "com.example.public",
            ExtensionVisibility::Public,
            "public-value",
        ),
        extension(
            "com.example.private_context",
            ExtensionVisibility::Private,
            "private-secret",
        ),
    ])
    .unwrap();
    let header = FrameHeader::new(
        FrameId::new("parent-frame").unwrap(),
        Timestamp::from_nanos(10),
        ClockDomain::new(
            ClockDomainId::new("capture.bytes").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(3),
        StreamId::new("stream-1").unwrap(),
        TraceId::new("trace-1").unwrap(),
        FrameType::Byte,
        metadata,
        extensions,
        Lineage::empty(),
    )
    .unwrap();
    Frame::new(
        header,
        FramePayload::Byte(ByteData::new(FrameBuffer::from_vec(vec![1, 2, 3]), None)),
    )
    .unwrap()
}

fn derivation(new_frame_id: &str, reason: &str) -> FrameDerivation {
    FrameDerivation::new(
        FrameId::new(new_frame_id).unwrap(),
        Timestamp::from_nanos(20),
        SequenceId::new(4),
        TransformOrigin::new(
            Some(NodeId::new("normalizer").unwrap()),
            Some(EdgeId::new("capture-to-normalizer").unwrap()),
        )
        .unwrap(),
        reason,
    )
    .unwrap()
}

#[test]
fn derivation_builder_rejects_an_invalid_lineage_reason() {
    let error = FrameDerivation::new(
        FrameId::new("invalid-derivation").unwrap(),
        Timestamp::from_nanos(20),
        SequenceId::new(4),
        TransformOrigin::new(Some(NodeId::new("normalizer").unwrap()), None).unwrap(),
        "secret\nreason",
    )
    .err()
    .unwrap();
    assert_eq!(error.code(), "MUXIVA-FRM-LINEAGE-REASON");
}

#[test]
fn derivation_preserves_parent_and_unknown_extensions_by_default() {
    let parent = frame_with_public_and_private_extensions();
    let original_extensions: Vec<_> = parent.header().extensions().iter().cloned().collect();
    let original_payload = parent.as_byte().unwrap().data().buffer().clone();

    let replacement = FrameBuffer::from_vec(vec![9, 8]);
    let child = parent
        .derive(
            derivation("child-frame", "normalize-bytes")
                .with_payload(FramePayload::Byte(ByteData::new(replacement.clone(), None))),
        )
        .unwrap();

    assert_eq!(parent.header().frame_id().as_str(), "parent-frame");
    assert_eq!(parent.as_byte().unwrap().data().buffer(), &original_payload);
    assert_eq!(
        parent.header().extensions().iter().collect::<Vec<_>>(),
        original_extensions.iter().collect::<Vec<_>>()
    );
    assert!(parent.header().lineage().is_empty());

    assert_eq!(child.header().stream_id(), parent.header().stream_id());
    assert_eq!(child.header().trace_id(), parent.header().trace_id());
    assert_eq!(
        child.header().clock_domain(),
        parent.header().clock_domain()
    );
    assert_eq!(child.header().metadata(), parent.header().metadata());
    assert_eq!(
        child.header().extensions().iter().collect::<Vec<_>>(),
        parent.header().extensions().iter().collect::<Vec<_>>()
    );
    assert_eq!(
        child
            .header()
            .extensions()
            .iter()
            .map(|item| item.key().as_str())
            .collect::<Vec<_>>(),
        ["com.example.public", "com.example.private_context"]
    );
    assert_eq!(child.header().lineage().len(), 1);
    let entry = child.header().lineage().iter().next().unwrap();
    assert_eq!(entry.parent_frame_id(), parent.header().frame_id());
    assert_eq!(entry.origin().node_id().unwrap().as_str(), "normalizer");
    assert_eq!(
        entry.origin().edge_id().unwrap().as_str(),
        "capture-to-normalizer"
    );
    assert_eq!(entry.reason(), "normalize-bytes");
    assert_eq!(child.as_byte().unwrap().data().buffer(), &replacement);
    assert_ne!(
        child.as_byte().unwrap().data().buffer().as_slice(),
        parent.as_byte().unwrap().data().buffer().as_slice()
    );

    let error = parent
        .derive(derivation("parent-frame", "duplicate-parent-id"))
        .unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-DERIVATION-ID");
}

#[test]
fn derivation_can_replace_metadata_extensions_and_frame_type() {
    let parent = frame_with_public_and_private_extensions();
    let metadata = Metadata::try_from_iter([("normalized", Value::Bool(true))]).unwrap();
    let extensions = Extensions::try_from_iter([extension(
        "com.example.replacement",
        ExtensionVisibility::Public,
        "replacement-value",
    )])
    .unwrap();

    let child = parent
        .derive(
            derivation("text-child", "decode-text")
                .with_metadata(metadata.clone())
                .with_extensions(extensions.clone())
                .with_payload(FramePayload::Text(TextData::new("decoded"))),
        )
        .unwrap();

    assert_eq!(child.frame_type(), FrameType::Text);
    assert_eq!(child.header().frame_type(), FrameType::Text);
    assert_eq!(child.as_text().unwrap().data().as_str(), "decoded");
    assert_eq!(child.header().metadata(), &metadata);
    assert_eq!(child.header().extensions(), &extensions);
    assert_eq!(parent.frame_type(), FrameType::Byte);
    assert_eq!(parent.header().extensions().len(), 2);
}

#[test]
fn derivation_without_payload_override_shares_the_parent_buffer() {
    let parent = frame_with_public_and_private_extensions();
    let child = parent
        .derive(derivation("preserved-payload-child", "preserve-payload"))
        .unwrap();

    assert_eq!(
        child.as_byte().unwrap().data().buffer().as_slice().as_ptr(),
        parent
            .as_byte()
            .unwrap()
            .data()
            .buffer()
            .as_slice()
            .as_ptr()
    );
}

#[test]
fn replacement_lineage_bridge_appends_exactly_one_edge_parent_entry() {
    let parent = frame_with_public_and_private_extensions();
    let policy_frame = parent
        .derive(
            derivation("policy-frame", "policy-private-lineage")
                .with_payload(FramePayload::Text(TextData::new("replacement"))),
        )
        .unwrap();
    assert_eq!(policy_frame.header().lineage().len(), 1);

    let replaced = parent
        .attach_replacement_lineage(
            policy_frame,
            TransformOrigin::new(None, Some(EdgeId::new("edge-replacement").unwrap())).unwrap(),
            "edge policy replacement",
        )
        .unwrap();

    assert_eq!(replaced.header().frame_id().as_str(), "policy-frame");
    assert_eq!(replaced.as_text().unwrap().data().as_str(), "replacement");
    assert_eq!(replaced.header().lineage().len(), 1);
    let entry = replaced.header().lineage().iter().next().unwrap();
    assert_eq!(entry.parent_frame_id(), parent.header().frame_id());
    assert_eq!(
        entry.origin().edge_id().unwrap().as_str(),
        "edge-replacement"
    );
    assert!(entry.origin().node_id().is_none());
    assert_eq!(entry.reason(), "edge policy replacement");

    let error = parent
        .attach_replacement_lineage(
            parent.clone(),
            TransformOrigin::new(None, Some(EdgeId::new("edge").unwrap())).unwrap(),
            "edge policy replacement",
        )
        .unwrap_err();
    assert_eq!(error.code(), "MUXIVA-FRM-REPLACEMENT-ID");
}

#[test]
fn private_extension_is_absent_from_default_views_privacy() {
    let frame = frame_with_public_and_private_extensions();
    let public_header: PublicFrameHeaderView<'_> = frame.public_view().header();
    let public_keys: Vec<_> = public_header
        .extensions()
        .map(|extension| extension.key().as_str())
        .collect();
    assert_eq!(public_keys, vec!["com.example.public"]);
    assert_eq!(public_header.frame_id(), frame.header().frame_id());
    assert_eq!(public_header.timestamp(), frame.header().timestamp());
    assert_eq!(public_header.clock_domain(), frame.header().clock_domain());
    assert_eq!(public_header.sequence_id(), frame.header().sequence_id());
    assert_eq!(public_header.stream_id(), frame.header().stream_id());
    assert_eq!(public_header.trace_id(), frame.header().trace_id());
    assert_eq!(public_header.metadata(), frame.header().metadata());
    assert_eq!(public_header.lineage(), frame.header().lineage());
    assert_eq!(frame.public_view().frame_type(), FrameType::Byte);

    for rendered in [format!("{frame:?}"), format!("{:?}", frame.header())] {
        for hidden in [
            "com.example.private_context",
            "private-secret",
            "public-value",
            "private-device-name",
            "capture-device",
        ] {
            assert!(!rendered.contains(hidden), "leaked {hidden}: {rendered}");
        }
    }

    let private = frame
        .header()
        .extensions()
        .iter()
        .find(|extension| extension.visibility() == ExtensionVisibility::Private)
        .unwrap();
    assert_eq!(private.key().as_str(), "com.example.private_context");
    assert_eq!(private.value(), &Value::String("private-secret".into()));
    let extension_rendered = format!("{private:?}");
    assert!(!extension_rendered.contains("com.example.private_context"));
    assert!(!extension_rendered.contains("private-secret"));
    assert!(extension_rendered.contains("<private>"));

    let public = frame.header().extensions().public_iter().next().unwrap();
    let public_rendered = format!("{public:?}");
    assert!(public_rendered.contains("com.example.public"));
    assert!(public_rendered.contains("SchemaVersion(7)"));
    assert!(public_rendered.contains("Node"));
    assert!(public_rendered.contains("Public"));
    assert!(!public_rendered.contains("public-value"));

    let log_safe = frame.log_safe_view();
    assert_eq!(log_safe.frame_id(), frame.header().frame_id());
    assert_eq!(log_safe.stream_id(), frame.header().stream_id());
    assert_eq!(log_safe.trace_id(), frame.header().trace_id());
    assert_eq!(log_safe.frame_type(), FrameType::Byte);
    assert_eq!(log_safe.timestamp(), frame.header().timestamp());
    assert_eq!(log_safe.clock_domain(), frame.header().clock_domain());
    assert_eq!(log_safe.sequence_id(), frame.header().sequence_id());
    assert_eq!(log_safe.payload_byte_len(), Some(3));
    assert_eq!(log_safe.metadata_key_count(), 1);
    assert_eq!(log_safe.public_extension_count(), 1);
    assert_eq!(log_safe.lineage_count(), 0);

    let buffer_rendered = format!("{:?}", frame.as_byte().unwrap().data().buffer());
    assert_eq!(buffer_rendered, "FrameBuffer { len: 3 }");
    assert!(!buffer_rendered.contains("1, 2, 3"));
}

#[test]
fn privacy_log_safe_payload_lengths_follow_payload_kind() {
    let byte_parent = frame_with_public_and_private_extensions();
    let text = byte_parent
        .derive(
            derivation("text-length", "decode")
                .with_payload(FramePayload::Text(TextData::new("hé"))),
        )
        .unwrap();
    assert_eq!(text.log_safe_view().payload_byte_len(), Some(3));
}
