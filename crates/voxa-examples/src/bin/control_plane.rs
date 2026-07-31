use std::{num::NonZeroUsize, sync::mpsc, time::Duration};

use voxa_core::{EventBus, ResourceKey, ResourceStore, TransportControl};
use voxa_types::{
    ClockDomain, ClockDomainId, ClockKind, EventData, Extensions, Frame, FrameHeader, FrameId,
    FramePayload, FrameType, Lineage, Metadata, NamespacedName, NodeId, SchemaVersion, SequenceId,
    StreamId, Timestamp, TraceId, TurnId, Value, ValueMap,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let control = TransportControl::new(TurnId::new("turn-1")?);
    let resources = ResourceStore::new();
    resources.insert(
        ResourceKey::new("transport.primary")?,
        std::sync::Arc::new(control.clone()),
    )?;

    let bus = EventBus::new(NonZeroUsize::new(8).expect("non-zero constant"));
    let topic = NamespacedName::new("voxa.transport.turn.interrupted")?;
    let (observed_tx, observed_rx) = mpsc::channel();
    bus.subscribe(topic.clone(), move |event| {
        observed_tx.send(event.data().topic().to_string()).ok();
        Ok(())
    })?;

    let payload = Value::Map(ValueMap::try_from_iter([(
        "turn_id",
        Value::String(Box::from("turn-1")),
    )])?);
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
            NodeId::new("transport.adapter")?,
            payload,
        )),
    )?;
    let event = event.as_event().expect("Event payload").clone();
    control.apply_event(&event)?;
    bus.publish(event)?;

    println!(
        "{} interrupted={}",
        observed_rx.recv_timeout(Duration::from_secs(1))?,
        control.snapshot().interrupted()
    );
    bus.stop(Duration::from_secs(1));
    resources.stop();
    Ok(())
}
