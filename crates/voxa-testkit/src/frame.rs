use voxa_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, EventData, Extensions, Frame,
    FrameBuffer, FrameHeader, FrameId, FramePayload, FrameType, Lineage, Metadata, NamespacedName,
    NodeId, PcmSampleFormat, SchemaVersion, SequenceId, SignalData, StreamId, TextData, Timestamp,
    TraceId, Value,
};
fn header(kind: FrameType, sequence: u64) -> FrameHeader {
    FrameHeader::new(
        FrameId::new(format!("test-{sequence}")).unwrap(),
        Timestamp::from_nanos(sequence as i64),
        ClockDomain::new(
            ClockDomainId::new("test.clock").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(sequence),
        StreamId::new("test.stream").unwrap(),
        TraceId::new("test.trace").unwrap(),
        kind,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap()
}
pub fn text_frame(sequence: u64, text: impl Into<String>) -> Frame {
    Frame::new(
        header(FrameType::Text, sequence),
        FramePayload::Text(TextData::new(text.into())),
    )
    .unwrap()
}
pub fn audio_frame(sequence: u64) -> Frame {
    Frame::new(
        header(FrameType::Audio, sequence),
        FramePayload::Audio(
            AudioData::new(
                FrameBuffer::from_vec(vec![0; 960]),
                48_000,
                1,
                PcmSampleFormat::I16Le,
                AudioLayout::Interleaved,
                480,
            )
            .unwrap(),
        ),
    )
    .unwrap()
}
pub fn signal_frame(sequence: u64, name: &str, source: &str) -> Frame {
    Frame::new(
        header(FrameType::Signal, sequence),
        FramePayload::Signal(SignalData::new(
            NamespacedName::new(name).unwrap(),
            SchemaVersion::new(1).unwrap(),
            NodeId::new(source).unwrap(),
            Value::Null,
        )),
    )
    .unwrap()
}
pub fn event_frame(sequence: u64, topic: &str, source: &str) -> Frame {
    Frame::new(
        header(FrameType::Event, sequence),
        FramePayload::Event(EventData::new(
            NamespacedName::new(topic).unwrap(),
            SchemaVersion::new(1).unwrap(),
            NodeId::new(source).unwrap(),
            Value::Null,
        )),
    )
    .unwrap()
}
