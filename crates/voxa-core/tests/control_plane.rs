use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use voxa_core::{
    ConcurrentRuntime, ConfigMap, ConfigSchema, ConnectionState, ControlApplyOutcome, DrainMode,
    EdgeDescriptor, EdgePolicies, EnabledCondition, EventBus, GraphBuilder, LifecycleCapabilities,
    Node, NodeContext, NodeDescriptor, NodeInstances, NodeKind, NodeTypeName, PortDescriptor,
    PortDirection, PortName, QueueOverflowPolicy, QueuePolicy, ResourceKey, ResourceStore,
    ResourceStoreError, RuntimeOptions, RuntimeWaitError, SignalEmissionError, TransformPolicy,
    TransportControl, ValidationPolicy, VisibilityDescriptor,
};
use voxa_types::{
    ClockDomain, ClockDomainId, ClockKind, EdgeId, EventData, EventFrame, Extensions, Frame,
    FrameHeader, FrameId, FramePayload, FrameType, Lineage, Metadata, NamespacedName, NodeId,
    SchemaVersion, SequenceId, SignalData, SignalFrame, StreamId, TextData, Timestamp, TraceId,
    TurnId, Value, VoxaError,
};

fn node_id(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn port(value: &str) -> PortName {
    PortName::new(value).unwrap()
}

fn header(sequence: u64, frame_type: FrameType, prefix: &str) -> FrameHeader {
    FrameHeader::new(
        FrameId::new(format!("{prefix}-{sequence}")).unwrap(),
        Timestamp::from_nanos(sequence as i64),
        ClockDomain::new(
            ClockDomainId::new("control.clock").unwrap(),
            ClockKind::Monotonic,
        ),
        SequenceId::new(sequence),
        StreamId::new("control.stream").unwrap(),
        TraceId::new("control.trace").unwrap(),
        frame_type,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap()
}

fn text_frame(sequence: u64) -> Frame {
    Frame::new(
        header(sequence, FrameType::Text, "text"),
        FramePayload::Text(TextData::new(sequence.to_string())),
    )
    .unwrap()
}

fn signal(sequence: u64, source: &str) -> SignalFrame {
    Frame::new(
        header(sequence, FrameType::Signal, "signal"),
        FramePayload::Signal(SignalData::new(
            NamespacedName::new("voxa.test.signal").unwrap(),
            SchemaVersion::new(1).unwrap(),
            node_id(source),
            Value::Integer(sequence as i64),
        )),
    )
    .unwrap()
    .as_signal()
    .unwrap()
    .clone()
}

fn event(sequence: u64, topic: &str) -> EventFrame {
    Frame::new(
        header(sequence, FrameType::Event, "event"),
        FramePayload::Event(EventData::new(
            NamespacedName::new(topic).unwrap(),
            SchemaVersion::new(1).unwrap(),
            node_id("publisher"),
            Value::Integer(sequence as i64),
        )),
    )
    .unwrap()
    .as_event()
    .unwrap()
    .clone()
}

fn descriptor(name: &str, kind: NodeKind, ports: &[(&str, PortDirection)]) -> NodeDescriptor {
    let id = node_id(name);
    NodeDescriptor::new(
        id.clone(),
        NodeTypeName::new(format!("test.{name}")).unwrap(),
        kind,
        ports
            .iter()
            .map(|(name, direction)| {
                PortDescriptor::new(id.clone(), port(name), *direction, FrameType::Text)
            })
            .collect::<Vec<_>>(),
        ConfigSchema::empty(),
        LifecycleCapabilities::new(true, true, true, true),
    )
}

fn connected_graph() -> voxa_core::GraphDefinition {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "source",
            NodeKind::Source,
            &[("out", PortDirection::Output)],
        ))
        .unwrap();
    builder
        .add_node(descriptor(
            "sink",
            NodeKind::Sink,
            &[("in", PortDirection::Input)],
        ))
        .unwrap();
    builder
        .connect(EdgeDescriptor::new(
            EdgeId::new("source-sink").unwrap(),
            node_id("source"),
            port("out"),
            node_id("sink"),
            port("in"),
            FrameType::Text,
            QueuePolicy::new(NonZeroUsize::new(16).unwrap(), QueueOverflowPolicy::Block),
            ValidationPolicy::TypeGateOnly,
            TransformPolicy::Identity,
            EnabledCondition::Always,
            VisibilityDescriptor::default(),
        ))
        .unwrap();
    builder.build().unwrap()
}

struct SignalSource {
    count: u64,
    thread_id: Arc<Mutex<Option<thread::ThreadId>>>,
}

impl Node for SignalSource {
    fn on_process(
        &mut self,
        _: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        *self.thread_id.lock().unwrap() = Some(thread::current().id());
        for sequence in 0..self.count {
            context.emit_signal(signal(sequence, "source"))?;
        }
        Ok(())
    }
}

struct SignalSink {
    received: Arc<Mutex<Vec<(u64, thread::ThreadId)>>>,
}

impl Node for SignalSink {
    fn on_process(&mut self, _: Option<Frame>, _: &mut NodeContext) -> voxa_types::Result<()> {
        Ok(())
    }

    fn on_signal(&mut self, signal: SignalFrame, _: &mut NodeContext) -> voxa_types::Result<()> {
        self.received
            .lock()
            .unwrap()
            .push((signal.header().sequence_id().get(), thread::current().id()));
        Ok(())
    }
}

#[test]
fn adjacent_signals_are_queued_ordered_and_cross_thread() {
    let source_thread = Arc::new(Mutex::new(None));
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        node_id("source"),
        Box::new(SignalSource {
            count: 8,
            thread_id: source_thread.clone(),
        }),
    );
    nodes.insert(
        node_id("sink"),
        Box::new(SignalSink {
            received: received.clone(),
        }),
    );
    let runtime = ConcurrentRuntime::new(
        connected_graph(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(2)).unwrap();

    let values = received.lock().unwrap();
    assert_eq!(
        values
            .iter()
            .map(|(sequence, _)| *sequence)
            .collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
    let source_thread = source_thread.lock().unwrap().unwrap();
    assert!(values
        .iter()
        .all(|(_, target_thread)| *target_thread != source_thread));
    let metrics = runtime
        .signal_metrics(&EdgeId::new("source-sink").unwrap())
        .unwrap();
    assert_eq!(metrics.enqueue_total, 8);
    assert_eq!(metrics.dequeue_total, 8);
}

#[test]
fn emitting_without_an_actual_edge_returns_a_structured_error() {
    let mut context = NodeContext::new(node_id("isolated"), ConfigMap::empty(), None);
    let error = context
        .emit_signal(signal(1, "isolated"))
        .expect_err("isolated signal must be rejected");
    assert!(matches!(
        error,
        SignalEmissionError::NoConnectedDownstream { .. }
    ));
    let error: VoxaError = error.into();
    assert_eq!(error.code(), "VOXA-SIGNAL-NO-EDGE");
}

#[test]
fn event_bus_subscribe_unsubscribe_slow_and_faulty_handlers_are_isolated() {
    let bus = EventBus::new(NonZeroUsize::new(1).unwrap());
    let topic = NamespacedName::new("voxa.test.events").unwrap();
    let (fast_tx, fast_rx) = mpsc::channel();
    let fast = bus
        .subscribe(topic.clone(), move |event| {
            fast_tx.send(event.header().sequence_id().get()).unwrap();
            Ok(())
        })
        .unwrap();
    let slow = bus
        .subscribe(topic.clone(), |_| {
            thread::sleep(Duration::from_millis(80));
            Ok(())
        })
        .unwrap();
    let faulty = bus
        .subscribe(topic, |event| {
            if event.header().sequence_id().get() == 0 {
                Err(VoxaError::new(
                    voxa_types::ErrorCategory::External,
                    "VOXA-TEST-EVENT-HANDLER",
                    "handler failed",
                ))
            } else {
                panic!("isolated handler panic")
            }
        })
        .unwrap();

    let started = Instant::now();
    let reports = (0..20)
        .map(|sequence| bus.publish(event(sequence, "voxa.test.events")).unwrap())
        .collect::<Vec<_>>();
    assert!(started.elapsed() < Duration::from_millis(50));
    assert!(reports.iter().any(|report| report.dropped_full > 0));
    assert_eq!(fast_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 0);
    assert!(bus.unsubscribe(fast));
    assert!(!bus.unsubscribe(fast));
    assert_eq!(
        bus.publish(event(100, "voxa.test.events")).unwrap().matched,
        2
    );

    thread::sleep(Duration::from_millis(120));
    let faulty_snapshot = bus.subscriber_snapshot(faulty).unwrap();
    assert!(faulty_snapshot.handler_errors + faulty_snapshot.handler_panics > 0);
    assert!(bus.subscriber_snapshot(slow).unwrap().dropped_full > 0);
    let stopped = bus.stop(Duration::from_secs(1));
    assert!(stopped.stopped_first);
    assert!(stopped.unfinished.is_empty());
    assert!(bus.publish(event(101, "voxa.test.events")).is_err());
}

#[test]
fn resource_store_reports_missing_and_wrong_types_and_cleans_up() {
    let store = ResourceStore::new();
    let key = ResourceKey::new("transport.primary").unwrap();
    assert!(matches!(
        store.get::<String>(&key),
        Err(ResourceStoreError::Missing { .. })
    ));
    store.insert(key.clone(), Arc::new(7_u64)).unwrap();
    assert!(matches!(
        store.get::<String>(&key),
        Err(ResourceStoreError::TypeMismatch { .. })
    ));
    assert_eq!(*store.get::<u64>(&key).unwrap(), 7);
    assert!(store.stop());
    assert!(store.is_stopped());
    assert!(matches!(
        store.get::<u64>(&key),
        Err(ResourceStoreError::Missing { .. })
    ));
}

#[test]
fn turn_snapshot_switch_stale_filter_and_interrupt_are_atomic_and_idempotent() {
    let turn_one = TurnId::new("turn-1").unwrap();
    let turn_two = TurnId::new("turn-2").unwrap();
    let control = TransportControl::new(turn_one.clone());
    assert_eq!(control.interrupt(&turn_one), ControlApplyOutcome::Applied);
    assert_eq!(
        control.interrupt(&turn_one),
        ControlApplyOutcome::AlreadyApplied
    );
    assert!(control.snapshot().interrupted());

    let old = control
        .stamp_frame(
            &text_frame(1),
            FrameId::new("turn-frame-1").unwrap(),
            node_id("source"),
        )
        .unwrap();
    assert_eq!(
        control.transition_turn(turn_two.clone()),
        ControlApplyOutcome::Applied
    );
    let snapshot = control.snapshot();
    assert_eq!(snapshot.turn_id(), &turn_two);
    assert!(!snapshot.interrupted());
    assert!(!snapshot.audio_ended());
    assert!(!control.should_deliver_to_sink(&old).unwrap());
    assert_eq!(control.stale_sink_drops(), 1);

    let current = control
        .stamp_frame(
            &text_frame(2),
            FrameId::new("turn-frame-2").unwrap(),
            node_id("source"),
        )
        .unwrap();
    assert!(control.should_deliver_to_sink(&current).unwrap());
    assert_eq!(control.interrupt(&turn_one), ControlApplyOutcome::StaleTurn);
    assert_eq!(
        control.set_connection(ConnectionState::Connected),
        ControlApplyOutcome::Applied
    );
}

struct TurnSource {
    control: TransportControl,
}

impl Node for TurnSource {
    fn on_process(
        &mut self,
        _: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let old = self
            .control
            .stamp_frame(
                &text_frame(10),
                FrameId::new("runtime-old-turn").unwrap(),
                node_id("source"),
            )
            .unwrap();
        self.control.transition_turn(TurnId::new("turn-2").unwrap());
        let current = self
            .control
            .stamp_frame(
                &text_frame(11),
                FrameId::new("runtime-current-turn").unwrap(),
                node_id("source"),
            )
            .unwrap();
        context.emit(port("out"), old)?;
        context.emit(port("out"), current)?;
        Ok(())
    }
}

struct CollectSink(Arc<Mutex<Vec<u64>>>);

impl Node for CollectSink {
    fn on_process(&mut self, frame: Option<Frame>, _: &mut NodeContext) -> voxa_types::Result<()> {
        self.0
            .lock()
            .unwrap()
            .push(frame.unwrap().header().sequence_id().get());
        Ok(())
    }
}

#[test]
fn concurrent_runtime_filters_old_turn_immediately_before_sink() {
    let control = TransportControl::new(TurnId::new("turn-1").unwrap());
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        node_id("source"),
        Box::new(TurnSource {
            control: control.clone(),
        }),
    );
    nodes.insert(node_id("sink"), Box::new(CollectSink(received.clone())));
    let runtime = ConcurrentRuntime::new(
        connected_graph(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::new(DrainMode::Discard, DrainMode::Discard),
    )
    .unwrap()
    .with_transport_control(control.clone())
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(2)).unwrap();
    assert_eq!(*received.lock().unwrap(), [11]);
    assert_eq!(control.stale_sink_drops(), 1);
}

#[test]
fn event_bus_stop_races_publish_without_panics_or_blocking() {
    let bus = EventBus::default();
    bus.subscribe(NamespacedName::new("voxa.test.race").unwrap(), |_| Ok(()))
        .unwrap();
    let publisher_bus = bus.clone();
    let publisher = thread::spawn(move || {
        for sequence in 0..2_000 {
            if publisher_bus
                .publish(event(sequence, "voxa.test.race"))
                .is_err()
            {
                break;
            }
        }
    });
    let report = bus.stop(Duration::from_secs(1));
    publisher.join().unwrap();
    assert!(report.unfinished.is_empty());
    assert!(!bus.stop(Duration::ZERO).stopped_first);
}

#[test]
fn isolated_runtime_signal_error_aborts_cleanly_during_stop_race() {
    struct Isolated;
    impl Node for Isolated {
        fn on_process(
            &mut self,
            _: Option<Frame>,
            context: &mut NodeContext,
        ) -> voxa_types::Result<()> {
            context.emit_signal(signal(1, "isolated"))?;
            Ok(())
        }
    }
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor("isolated", NodeKind::Source, &[]))
        .unwrap();
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(node_id("isolated"), Box::new(Isolated));
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    let stopper = runtime.clone();
    let stop = thread::spawn(move || stopper.stop());
    let _ = stop.join().unwrap();
    assert!(matches!(
        runtime.wait(Duration::from_secs(2)),
        Err(RuntimeWaitError::Aborted(_))
    ));
}
