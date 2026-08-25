use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Barrier, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use muxiva_core::{
    ConcurrentRuntime, ConfigSchema, DrainMode, EdgeDescriptor, EdgePolicies, EdgeQueue,
    EnabledCondition, EnqueueOutcome, FrameObservation, FrameObservationDirection, GraphBuilder,
    LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeInstances, NodeKind,
    NodeTypeName, PortDescriptor, PortDirection, PortName, QueueDropReason, QueueOverflowPolicy,
    QueuePolicy, QueuePushError, RuntimeObserver, RuntimeOptions, RuntimeWaitError,
    SignalObservation, SignalObservationDirection, StopToken, TransformPolicy, ValidationPolicy,
    VisibilityDescriptor,
};
use muxiva_types::{
    ClockDomain, ClockDomainId, ClockKind, EdgeId, ErrorCategory, Extensions, Frame, FrameHeader,
    FrameId, FramePayload, FrameType, Lineage, Metadata, MuxivaError, NamespacedName, NodeId,
    SchemaVersion, SequenceId, SignalData, SignalFrame, StreamId, TextData, Timestamp, TraceId,
    Value,
};

fn id(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn edge_id(value: &str) -> EdgeId {
    EdgeId::new(value).unwrap()
}

fn port(value: &str) -> PortName {
    PortName::new(value).unwrap()
}

fn text_frame(sequence: u64) -> Frame {
    let header = FrameHeader::new(
        FrameId::new(format!("frame-{sequence}")).unwrap(),
        Timestamp::from_nanos(sequence as i64),
        ClockDomain::new(
            ClockDomainId::new("test-clock").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(sequence),
        StreamId::new("stream").unwrap(),
        TraceId::new("trace").unwrap(),
        FrameType::Text,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    Frame::new(
        header,
        FramePayload::Text(TextData::new(sequence.to_string())),
    )
    .unwrap()
}

fn signal_frame(sequence: u64, source: &str) -> SignalFrame {
    let header = FrameHeader::new(
        FrameId::new(format!("signal-{sequence}")).unwrap(),
        Timestamp::from_nanos(sequence as i64),
        ClockDomain::new(
            ClockDomainId::new("test-clock").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(sequence),
        StreamId::new("signal-stream").unwrap(),
        TraceId::new("signal-trace").unwrap(),
        FrameType::Signal,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    let frame = Frame::new(
        header,
        FramePayload::Signal(SignalData::new(
            NamespacedName::new("muxiva.voice.speech.started").unwrap(),
            SchemaVersion::new(1).unwrap(),
            id(source),
            Value::Null,
        )),
    )
    .unwrap();
    let Frame::Signal(signal) = frame else {
        unreachable!()
    };
    signal
}

#[test]
fn block_waiters_are_woken_by_close() {
    let queue = EdgeQueue::new(
        edge_id("edge"),
        QueuePolicy::new(NonZeroUsize::new(1).unwrap(), QueueOverflowPolicy::Block),
    );
    queue.push(text_frame(1)).unwrap();
    let producer_queue = queue.clone();
    let (producer_tx, producer_rx) = mpsc::channel();
    thread::spawn(move || {
        producer_tx
            .send(producer_queue.push(text_frame(2)))
            .unwrap()
    });
    assert!(producer_rx.recv_timeout(Duration::from_millis(30)).is_err());
    queue.close(DrainMode::Discard);
    assert_eq!(
        producer_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(QueuePushError::Closed)
    );

    let empty = EdgeQueue::new(edge_id("empty"), QueuePolicy::default());
    let consumer_queue = empty.clone();
    let (consumer_tx, consumer_rx) = mpsc::channel();
    thread::spawn(move || consumer_tx.send(consumer_queue.pop()).unwrap());
    assert!(consumer_rx.recv_timeout(Duration::from_millis(30)).is_err());
    empty.close(DrainMode::Drain);
    assert!(consumer_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
}

#[test]
fn discard_escalates_an_already_draining_queue() {
    let queue = EdgeQueue::new(
        edge_id("edge"),
        QueuePolicy::new(NonZeroUsize::new(2).unwrap(), QueueOverflowPolicy::Block),
    );
    queue.push(text_frame(1)).unwrap();
    queue.push(text_frame(2)).unwrap();
    queue.close(DrainMode::Drain);
    assert_eq!(queue.snapshot().queue_len(), 2);

    queue.close(DrainMode::Discard);
    assert!(queue.pop().is_err());
    let metrics = queue.snapshot();
    assert_eq!(metrics.queue_len(), 0);
    assert_eq!(metrics.drop_total(), 2);
    assert!(metrics.latest_error_reason().unwrap().contains("discarded"));
}

#[test]
fn drop_oldest_and_newest_are_explicit_and_metered() {
    let oldest = EdgeQueue::new(
        edge_id("oldest"),
        QueuePolicy::new(
            NonZeroUsize::new(1).unwrap(),
            QueueOverflowPolicy::DropOldest,
        ),
    );
    oldest.push(text_frame(1)).unwrap();
    assert_eq!(
        oldest.push(text_frame(2)).unwrap(),
        EnqueueOutcome::EnqueuedAfterDroppingOldest
    );
    assert_eq!(oldest.pop().unwrap().header().sequence_id().get(), 2);
    assert_eq!(oldest.snapshot().drop_total(), 1);

    let newest = EdgeQueue::new(
        edge_id("newest"),
        QueuePolicy::new(
            NonZeroUsize::new(1).unwrap(),
            QueueOverflowPolicy::DropNewest,
        ),
    );
    newest.push(text_frame(1)).unwrap();
    assert_eq!(
        newest.push(text_frame(2)).unwrap(),
        EnqueueOutcome::Dropped(QueueDropReason::QueueFullDropNewest)
    );
    assert_eq!(newest.pop().unwrap().header().sequence_id().get(), 1);
    let metrics = newest.snapshot();
    assert_eq!(metrics.drop_total(), 1);
    assert_eq!(metrics.full_total(), 1);
    assert!(metrics.latest_error_reason().unwrap().contains("newest"));
}

#[test]
fn stop_token_is_cross_thread_waking_and_idempotent() {
    let token = StopToken::new();
    let waiter = token.clone();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        waiter.wait();
        tx.send(()).unwrap();
    });
    assert!(token.cancel());
    assert!(!token.cancel());
    rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let concurrent = StopToken::new();
    let callers = (0..8)
        .map(|_| {
            let token = concurrent.clone();
            thread::spawn(move || token.cancel())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        callers
            .into_iter()
            .map(|caller| caller.join().unwrap())
            .filter(|cancelled| *cancelled)
            .count(),
        1
    );
}

fn descriptor(name: &str, kind: NodeKind, ports: &[(&str, PortDirection)]) -> NodeDescriptor {
    let node_id = id(name);
    NodeDescriptor::new(
        node_id.clone(),
        NodeTypeName::new(format!("test.{name}")).unwrap(),
        kind,
        ports
            .iter()
            .map(|(name, direction)| {
                PortDescriptor::new(node_id.clone(), port(name), *direction, FrameType::Text)
            })
            .collect::<Vec<_>>(),
        ConfigSchema::empty(),
        LifecycleCapabilities::new(true, true, true, true),
    )
}

fn connect(
    builder: &mut GraphBuilder,
    edge_name: &str,
    source: &str,
    output: &str,
    sink: &str,
    input: &str,
    capacity: usize,
) {
    builder
        .connect(EdgeDescriptor::new(
            edge_id(edge_name),
            id(source),
            port(output),
            id(sink),
            port(input),
            FrameType::Text,
            QueuePolicy::new(
                NonZeroUsize::new(capacity).unwrap(),
                QueueOverflowPolicy::Block,
            ),
            ValidationPolicy::TypeGateOnly,
            TransformPolicy::Identity,
            EnabledCondition::Always,
            VisibilityDescriptor::default(),
        ))
        .unwrap();
}

struct SourceNode {
    count: u64,
    thread_ids: Option<Arc<Mutex<Vec<thread::ThreadId>>>>,
}

impl Node for SourceNode {
    fn on_prepare(&mut self, _: &mut NodeContext) -> muxiva_types::Result<()> {
        if let Some(ids) = &self.thread_ids {
            ids.lock().unwrap().push(thread::current().id());
        }
        Ok(())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        assert!(input.is_none());
        if let Some(ids) = &self.thread_ids {
            ids.lock().unwrap().push(thread::current().id());
        }
        for sequence in 0..self.count {
            context.emit(port("out"), text_frame(sequence))?;
        }
        Ok(())
    }

    fn on_finish(&mut self, _: &mut NodeContext) -> muxiva_types::Result<()> {
        if let Some(ids) = &self.thread_ids {
            ids.lock().unwrap().push(thread::current().id());
        }
        Ok(())
    }
}

struct SinkNode {
    received: Arc<Mutex<Vec<u64>>>,
    delay: Duration,
    fail: bool,
    thread_ids: Option<Arc<Mutex<Vec<thread::ThreadId>>>>,
}

impl Node for SinkNode {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        _: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        if let Some(ids) = &self.thread_ids {
            ids.lock().unwrap().push(thread::current().id());
        }
        if self.fail {
            return Err(MuxivaError::new(
                ErrorCategory::Lifecycle,
                "MUXIVA-TEST-FIRST-ERROR",
                "sink failed",
            ));
        }
        thread::sleep(self.delay);
        self.received
            .lock()
            .unwrap()
            .push(input.unwrap().header().sequence_id().get());
        Ok(())
    }
}

fn source_sink_runtime(
    count: u64,
    capacity: usize,
    delay: Duration,
) -> (muxiva_core::GraphRuntime, Arc<Mutex<Vec<u64>>>) {
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
    connect(
        &mut builder,
        "edge",
        "source",
        "out",
        "sink",
        "in",
        capacity,
    );
    let graph = builder.build().unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(SourceNode {
            count,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("sink"),
        Box::new(SinkNode {
            received: received.clone(),
            delay,
            fail: false,
            thread_ids: None,
        }),
    );
    let runtime =
        ConcurrentRuntime::new(graph, nodes, EdgePolicies::new(), RuntimeOptions::default())
            .unwrap()
            .start()
            .unwrap();
    (runtime, received)
}

struct DeferredTransform {
    pending: Option<Frame>,
}

impl Node for DeferredTransform {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        if let Some(frame) = input {
            self.pending = Some(frame);
            context.schedule_next_tick(Duration::from_millis(5));
        } else if let Some(frame) = self.pending.take() {
            context.emit(port("out"), frame)?;
        }
        Ok(())
    }
}

#[test]
fn consumer_node_can_schedule_an_internal_callback_without_a_clock_edge() {
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
            "deferred",
            NodeKind::Transform,
            &[("in", PortDirection::Input), ("out", PortDirection::Output)],
        ))
        .unwrap();
    builder
        .add_node(descriptor(
            "sink",
            NodeKind::Sink,
            &[("in", PortDirection::Input)],
        ))
        .unwrap();
    connect(
        &mut builder,
        "source-deferred",
        "source",
        "out",
        "deferred",
        "in",
        2,
    );
    connect(
        &mut builder,
        "deferred-sink",
        "deferred",
        "out",
        "sink",
        "in",
        2,
    );

    let received = Arc::new(Mutex::new(Vec::new()));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(SourceNode {
            count: 1,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("deferred"),
        Box::new(DeferredTransform { pending: None }),
    );
    nodes.insert(
        id("sink"),
        Box::new(SinkNode {
            received: received.clone(),
            delay: Duration::ZERO,
            fail: false,
            thread_ids: None,
        }),
    );

    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(1)).unwrap();
    assert_eq!(*received.lock().unwrap(), vec![0]);
}

#[derive(Default)]
struct RecordingFrameObserver {
    observations: Mutex<Vec<(String, String, FrameObservationDirection, u64)>>,
    signals: Mutex<Vec<SignalObservationRecord>>,
}

type SignalObservationRecord = (String, Option<String>, SignalObservationDirection, u64);

impl RuntimeObserver for RecordingFrameObserver {
    fn observe_frame(&self, observation: FrameObservation<'_>) {
        self.observations.lock().unwrap().push((
            observation.node_id().as_str().to_owned(),
            observation.port().as_str().to_owned(),
            observation.direction(),
            observation.frame().header().sequence_id().get(),
        ));
    }

    fn observe_signal(&self, observation: SignalObservation<'_>) {
        self.signals.lock().unwrap().push((
            observation.node_id().as_str().to_owned(),
            observation.port().map(|port| port.as_str().to_owned()),
            observation.direction(),
            observation.signal().header().sequence_id().get(),
        ));
    }
}

#[test]
fn frame_observer_sees_each_node_input_and_output_boundary_once() {
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
    connect(&mut builder, "edge", "source", "out", "sink", "in", 4);
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(SourceNode {
            count: 3,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("sink"),
        Box::new(SinkNode {
            received: Arc::new(Mutex::new(Vec::new())),
            delay: Duration::ZERO,
            fail: false,
            thread_ids: None,
        }),
    );
    let observer = Arc::new(RecordingFrameObserver::default());
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .with_runtime_observer(observer.clone())
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(1)).unwrap();

    let observations = observer.observations.lock().unwrap();
    for sequence in 0..3 {
        assert_eq!(
            observations
                .iter()
                .filter(|value| {
                    value
                        == &&(
                            "source".into(),
                            "out".into(),
                            FrameObservationDirection::Output,
                            sequence,
                        )
                })
                .count(),
            1
        );
        assert_eq!(
            observations
                .iter()
                .filter(|value| {
                    value
                        == &&(
                            "sink".into(),
                            "in".into(),
                            FrameObservationDirection::Input,
                            sequence,
                        )
                })
                .count(),
            1
        );
    }
    assert_eq!(observations.len(), 6);
}

struct SignalSource;

impl Node for SignalSource {
    fn on_process(
        &mut self,
        _: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        context.emit_signal(signal_frame(7, "signal-source"))?;
        Ok(())
    }
}

struct SignalSink {
    received: Arc<AtomicU64>,
}

impl Node for SignalSink {
    fn on_process(&mut self, _: Option<Frame>, _: &mut NodeContext) -> muxiva_types::Result<()> {
        Ok(())
    }

    fn on_signal(
        &mut self,
        _: muxiva_types::SignalFrame,
        _: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.received.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[test]
fn runtime_observer_sees_signal_emission_and_delivery_boundaries() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "signal-source",
            NodeKind::Source,
            &[("out", PortDirection::Output)],
        ))
        .unwrap();
    builder
        .add_node(descriptor(
            "signal-sink",
            NodeKind::Sink,
            &[("in", PortDirection::Input)],
        ))
        .unwrap();
    connect(
        &mut builder,
        "signal-edge",
        "signal-source",
        "out",
        "signal-sink",
        "in",
        2,
    );
    let received = Arc::new(AtomicU64::new(0));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(id("signal-source"), Box::new(SignalSource));
    nodes.insert(
        id("signal-sink"),
        Box::new(SignalSink {
            received: received.clone(),
        }),
    );
    let observer = Arc::new(RecordingFrameObserver::default());
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .with_runtime_observer(observer.clone())
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(1)).unwrap();
    assert_eq!(received.load(Ordering::Relaxed), 1);
    assert_eq!(
        observer.signals.lock().unwrap().as_slice(),
        &[
            (
                "signal-source".into(),
                None,
                SignalObservationDirection::Output,
                7,
            ),
            (
                "signal-sink".into(),
                Some("in".into()),
                SignalObservationDirection::Input,
                7,
            ),
        ]
    );
}

struct SignalThenFrameSource;

impl Node for SignalThenFrameSource {
    fn on_process(
        &mut self,
        _: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        context.emit_signal(signal_frame(8, "ordered-source"))?;
        context.emit(port("out"), text_frame(8))?;
        Ok(())
    }
}

struct OrderedSignalSink {
    received: Arc<Mutex<Vec<&'static str>>>,
}

impl Node for OrderedSignalSink {
    fn on_process(
        &mut self,
        _: Option<Frame>,
        _: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.received.lock().unwrap().push("frame");
        Ok(())
    }

    fn on_signal(
        &mut self,
        _: SignalFrame,
        _: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.received.lock().unwrap().push("signal");
        Ok(())
    }
}

#[test]
fn callback_signals_are_delivered_before_accompanying_frames() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "ordered-source",
            NodeKind::Source,
            &[("out", PortDirection::Output)],
        ))
        .unwrap();
    builder
        .add_node(descriptor(
            "ordered-sink",
            NodeKind::Sink,
            &[("in", PortDirection::Input)],
        ))
        .unwrap();
    connect(
        &mut builder,
        "ordered-edge",
        "ordered-source",
        "out",
        "ordered-sink",
        "in",
        2,
    );
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(id("ordered-source"), Box::new(SignalThenFrameSource));
    nodes.insert(
        id("ordered-sink"),
        Box::new(OrderedSignalSink {
            received: received.clone(),
        }),
    );
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(1)).unwrap();
    assert_eq!(received.lock().unwrap().as_slice(), &["signal", "frame"]);
}

#[test]
fn block_policy_backpressures_a_slow_sink() {
    let (runtime, received) = source_sink_runtime(30, 2, Duration::from_millis(2));
    runtime.wait(Duration::from_secs(3)).unwrap();
    assert_eq!(received.lock().unwrap().len(), 30);
    let metrics = runtime.edge_metrics(&edge_id("edge")).unwrap();
    assert_eq!(metrics.enqueue_total(), 30);
    assert_eq!(metrics.dequeue_total(), 30);
    assert_eq!(metrics.drop_total(), 0);
    assert!(metrics.full_total() > 0);
    assert!(metrics.blocked_duration_ns() > 0);
    assert!(metrics.high_watermark() <= 2);
    assert!(metrics.payload_bytes_total() > 0);
}

#[test]
fn node_reported_counters_and_gauges_are_visible_in_runtime_metrics() {
    struct InstrumentedSource;
    impl Node for InstrumentedSource {
        fn on_process(
            &mut self,
            _: Option<Frame>,
            context: &mut NodeContext,
        ) -> muxiva_types::Result<()> {
            context.increment_counter("input.frames", 3).unwrap();
            context.increment_counter("input.frames", 2).unwrap();
            context.set_gauge("input.queue_duration_ms", 740).unwrap();
            context.set_gauge("input.queue_duration_ms", 120).unwrap();
            Ok(())
        }
    }

    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor("source", NodeKind::Source, &[]))
        .unwrap();
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(id("source"), Box::new(InstrumentedSource));
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(1)).unwrap();

    let metrics = runtime.node_metrics(&id("source")).unwrap();
    let values = metrics
        .custom_metrics()
        .iter()
        .map(|metric| (metric.name(), metric.kind(), metric.value()))
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        vec![
            ("input.frames", muxiva_core::NodeMetricKind::Counter, 5),
            (
                "input.queue_duration_ms",
                muxiva_core::NodeMetricKind::Gauge,
                120
            ),
        ]
    );
    assert_eq!(metrics.process_total(), 1);
    assert!(metrics.process_duration_ns() > 0);
}

#[test]
fn block_delivers_ten_thousand_frames_without_loss() {
    let (runtime, received) = source_sink_runtime(10_000, 8, Duration::ZERO);
    runtime.wait(Duration::from_secs(10)).unwrap();
    let values = received.lock().unwrap();
    assert_eq!(values.len(), 10_000);
    assert!(values.iter().copied().eq(0..10_000));
    assert_eq!(
        runtime.edge_metrics(&edge_id("edge")).unwrap().drop_total(),
        0
    );
}

#[test]
fn two_sources_run_concurrently_and_share_one_admitted_sink() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "source-a",
            NodeKind::Source,
            &[("out", PortDirection::Output)],
        ))
        .unwrap();
    builder
        .add_node(descriptor(
            "source-b",
            NodeKind::Source,
            &[("out", PortDirection::Output)],
        ))
        .unwrap();
    builder
        .add_node(descriptor(
            "sink",
            NodeKind::Sink,
            &[("a", PortDirection::Input), ("b", PortDirection::Input)],
        ))
        .unwrap();
    connect(&mut builder, "edge-a", "source-a", "out", "sink", "a", 3);
    connect(&mut builder, "edge-b", "source-b", "out", "sink", "b", 3);
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source-a"),
        Box::new(SourceNode {
            count: 100,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("source-b"),
        Box::new(SourceNode {
            count: 100,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("sink"),
        Box::new(SinkNode {
            received: received.clone(),
            delay: Duration::ZERO,
            fail: false,
            thread_ids: None,
        }),
    );
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(3)).unwrap();
    assert_eq!(received.lock().unwrap().len(), 200);
}

#[test]
fn first_node_error_stops_and_aborts_the_graph() {
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
    connect(&mut builder, "edge", "source", "out", "sink", "in", 1);
    let received = Arc::new(Mutex::new(Vec::new()));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(SourceNode {
            count: 100,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("sink"),
        Box::new(SinkNode {
            received,
            delay: Duration::ZERO,
            fail: true,
            thread_ids: None,
        }),
    );
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    match runtime.wait(Duration::from_secs(2)) {
        Err(RuntimeWaitError::Aborted(reason)) => {
            assert_eq!(reason.root().code(), "MUXIVA-TEST-FIRST-ERROR")
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn stop_is_idempotent_and_unblocks_a_backpressured_source() {
    let (runtime, _) = source_sink_runtime(1_000, 1, Duration::from_millis(5));
    thread::sleep(Duration::from_millis(10));
    let started = Instant::now();
    assert!(runtime.stop());
    assert!(!runtime.stop());
    assert!(matches!(
        runtime.wait(Duration::from_secs(2)),
        Err(RuntimeWaitError::Aborted(_))
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn no_user_node_code_runs_on_the_starting_caller_thread() {
    let caller = thread::current().id();
    let ids = Arc::new(Mutex::new(Vec::new()));
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
    connect(&mut builder, "edge", "source", "out", "sink", "in", 1);
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(SourceNode {
            count: 1,
            thread_ids: Some(ids.clone()),
        }),
    );
    nodes.insert(
        id("sink"),
        Box::new(SinkNode {
            received: Arc::new(Mutex::new(Vec::new())),
            delay: Duration::ZERO,
            fail: false,
            thread_ids: Some(ids.clone()),
        }),
    );
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(2)).unwrap();
    let observed = ids.lock().unwrap();
    assert!(!observed.is_empty());
    assert!(observed.iter().all(|id| *id != caller));
}

struct MaliciousSource;

impl Node for MaliciousSource {
    fn on_process(
        &mut self,
        _: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        for sequence in 0..1_000 {
            let _ = context.emit(port("out"), text_frame(sequence));
        }
        Ok(())
    }
}

#[test]
fn ignored_emission_errors_still_bound_and_abort_a_malicious_source() {
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
    connect(&mut builder, "edge", "source", "out", "sink", "in", 1);
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(id("source"), Box::new(MaliciousSource));
    nodes.insert(
        id("sink"),
        Box::new(SinkNode {
            received: Arc::new(Mutex::new(Vec::new())),
            delay: Duration::ZERO,
            fail: false,
            thread_ids: None,
        }),
    );
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default().with_emission_budget(NonZeroUsize::new(8).unwrap()),
    )
    .unwrap()
    .start()
    .unwrap();
    let Err(RuntimeWaitError::Aborted(reason)) = runtime.wait(Duration::from_secs(2)) else {
        panic!("emission overflow did not abort");
    };
    assert_eq!(reason.root().code(), "MUXIVA-CONCURRENT-EMISSION-LIMIT");
    assert_eq!(
        reason.root().details().get("emission_limit"),
        Some(&muxiva_types::Value::String(Box::from("8")))
    );
}

struct GateSink {
    entered: mpsc::Sender<()>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl Node for GateSink {
    fn on_process(&mut self, _: Option<Frame>, _: &mut NodeContext) -> muxiva_types::Result<()> {
        let _ = self.entered.send(());
        let (lock, changed) = &*self.release;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = changed.wait(released).unwrap();
        }
        Ok(())
    }
}

#[test]
fn blocked_slow_branch_does_not_withhold_bypass_batch() {
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
            "asr",
            NodeKind::Sink,
            &[("in", PortDirection::Input)],
        ))
        .unwrap();
    builder
        .add_node(descriptor(
            "bypass",
            NodeKind::Sink,
            &[("in", PortDirection::Input)],
        ))
        .unwrap();
    connect(&mut builder, "asr-edge", "source", "out", "asr", "in", 1);
    connect(
        &mut builder,
        "bypass-edge",
        "source",
        "out",
        "bypass",
        "in",
        1,
    );
    let bypass = Arc::new(Mutex::new(Vec::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(SourceNode {
            count: 64,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("asr"),
        Box::new(GateSink {
            entered: entered_tx,
            release: release.clone(),
        }),
    );
    nodes.insert(
        id("bypass"),
        Box::new(SinkNode {
            received: bypass.clone(),
            delay: Duration::ZERO,
            fail: false,
            thread_ids: None,
        }),
    );
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while bypass.lock().unwrap().len() != 64 && Instant::now() < deadline {
        thread::yield_now();
    }
    assert_eq!(bypass.lock().unwrap().len(), 64);
    assert!(runtime.stop());
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    assert!(matches!(
        runtime.wait(Duration::from_secs(2)),
        Err(RuntimeWaitError::Aborted(_))
    ));
}

struct RacingFailureSink {
    barrier: Arc<Barrier>,
    code: &'static str,
}

impl Node for RacingFailureSink {
    fn on_process(&mut self, _: Option<Frame>, _: &mut NodeContext) -> muxiva_types::Result<()> {
        self.barrier.wait();
        Err(MuxivaError::new(
            ErrorCategory::Lifecycle,
            self.code,
            "competing failure",
        ))
    }
}

#[test]
fn competing_stop_and_node_errors_publish_one_stable_terminal_outcome() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "source",
            NodeKind::Source,
            &[("out", PortDirection::Output)],
        ))
        .unwrap();
    for sink in ["sink-a", "sink-b"] {
        builder
            .add_node(descriptor(
                sink,
                NodeKind::Sink,
                &[("in", PortDirection::Input)],
            ))
            .unwrap();
        connect(
            &mut builder,
            &format!("edge-{sink}"),
            "source",
            "out",
            sink,
            "in",
            1,
        );
    }
    let barrier = Arc::new(Barrier::new(3));
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(SourceNode {
            count: 1,
            thread_ids: None,
        }),
    );
    nodes.insert(
        id("sink-a"),
        Box::new(RacingFailureSink {
            barrier: barrier.clone(),
            code: "MUXIVA-TEST-RACE-A",
        }),
    );
    nodes.insert(
        id("sink-b"),
        Box::new(RacingFailureSink {
            barrier: barrier.clone(),
            code: "MUXIVA-TEST-RACE-B",
        }),
    );
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
    let stop_thread = thread::spawn(move || {
        barrier.wait();
        stopper.stop()
    });
    let Err(RuntimeWaitError::Aborted(first)) = runtime.wait(Duration::from_secs(2)) else {
        panic!("race did not abort");
    };
    stop_thread.join().unwrap();
    let Err(RuntimeWaitError::Aborted(second)) = runtime.wait(Duration::ZERO) else {
        panic!("terminal result changed");
    };
    assert_eq!(first, second);
    assert!(matches!(
        first.root().code(),
        "MUXIVA-TEST-RACE-A" | "MUXIVA-TEST-RACE-B" | "MUXIVA-CONCURRENT-CANCELLED"
    ));
    assert_eq!(
        runtime.state(),
        muxiva_core::ConcurrentRuntimeState::Aborted
    );
    assert!(!runtime.stop());
}

struct BlockingFinishSource {
    entered: mpsc::Sender<()>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl Node for BlockingFinishSource {
    fn on_process(&mut self, _: Option<Frame>, _: &mut NodeContext) -> muxiva_types::Result<()> {
        Ok(())
    }

    fn on_finish(&mut self, _: &mut NodeContext) -> muxiva_types::Result<()> {
        self.entered.send(()).unwrap();
        let mut released = self.release.0.lock().unwrap();
        while !*released {
            released = self.release.1.wait(released).unwrap();
        }
        Ok(())
    }
}

#[test]
fn stop_linearizes_against_finish_and_reports_lifecycle_as_active() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor("source", NodeKind::Source, &[]))
        .unwrap();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(
        id("source"),
        Box::new(BlockingFinishSource {
            entered: entered_tx,
            release: release.clone(),
        }),
    );
    let runtime = ConcurrentRuntime::new(
        builder.build().unwrap(),
        nodes,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let Err(RuntimeWaitError::Timeout(diagnostics)) = runtime.wait(Duration::from_millis(10))
    else {
        panic!("blocked finish was not reported");
    };
    assert_eq!(diagnostics.active_nodes(), &[id("source")]);
    assert!(runtime.stop());
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    assert!(matches!(
        runtime.wait(Duration::from_secs(2)),
        Err(RuntimeWaitError::Aborted(_))
    ));
    assert_eq!(
        runtime.state(),
        muxiva_core::ConcurrentRuntimeState::Aborted
    );
}
