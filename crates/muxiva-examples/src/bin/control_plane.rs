use std::{num::NonZeroUsize, sync::mpsc, time::Duration};

use muxiva_core::EventBus;
use muxiva_types::{
    ClockDomain, ClockDomainId, ClockKind, EventData, Extensions, Frame, FrameHeader, FrameId,
    FramePayload, FrameType, Lineage, Metadata, NamespacedName, NodeId, SchemaVersion, SequenceId,
    StreamId, Timestamp, TraceId, Value,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bus = EventBus::new(NonZeroUsize::new(8).expect("non-zero constant"));
    let topic = NamespacedName::new("muxiva.voice.speech.started")?;
    let (observed_tx, observed_rx) = mpsc::channel();
    bus.subscribe(topic.clone(), move |event| {
        observed_tx.send(event.data().topic().to_string()).ok();
        Ok(())
    })?;
    let header = FrameHeader::new(
        FrameId::new("event-1")?,
        Timestamp::from_nanos(1),
        ClockDomain::new(ClockDomainId::new("control.clock")?, ClockKind::Monotonic),
        SequenceId::new(1),
        StreamId::new("control.stream")?,
        TraceId::new("control.trace")?,
        FrameType::Event,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )?;
    let event = Frame::new(
        header,
        FramePayload::Event(EventData::new(
            topic,
            SchemaVersion::new(1)?,
            NodeId::new("voice.vad")?,
            Value::Bool(true),
        )),
    )?;
    bus.publish(event.as_event().expect("Event payload").clone())?;
    println!("{}", observed_rx.recv_timeout(Duration::from_secs(1))?);
    bus.stop(Duration::from_secs(1));
    Ok(())
}
