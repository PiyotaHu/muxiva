use std::{
    collections::BTreeMap,
    num::NonZeroUsize,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use voxa_core::{
    ConcurrentRuntime, ConfigSchema, DrainMode, EdgeDescriptor, EdgePolicies, EdgeQueue,
    EnabledCondition, EnqueueOutcome, GraphBuilder, LifecycleCapabilities, Node, NodeContext,
    NodeDescriptor, NodeInstances, NodeKind, NodeTypeName, PortDescriptor, PortDirection, PortName,
    QueueDropReason, QueueOverflowPolicy, QueuePolicy, QueuePushError, RuntimeOptions,
    RuntimeWaitError, StopToken, TransformPolicy, ValidationPolicy, VisibilityDescriptor,
};
use voxa_types::{
    ClockDomain, ClockDomainId, ClockKind, EdgeId, ErrorCategory, Extensions, Frame, FrameHeader,
    FrameId, FramePayload, FrameType, Lineage, Metadata, NodeId, SequenceId, StreamId, TextData,
    Timestamp, TraceId, VoxaError,
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
    fn on_prepare(&mut self, _: &mut NodeContext) -> voxa_types::Result<()> {
        if let Some(ids) = &self.thread_ids {
            ids.lock().unwrap().push(thread::current().id());
        }
        Ok(())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        assert!(input.is_none());
        if let Some(ids) = &self.thread_ids {
            ids.lock().unwrap().push(thread::current().id());
        }
        for sequence in 0..self.count {
            context.emit(port("out"), text_frame(sequence));
        }
        Ok(())
    }

    fn on_finish(&mut self, _: &mut NodeContext) -> voxa_types::Result<()> {
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
    fn on_process(&mut self, input: Option<Frame>, _: &mut NodeContext) -> voxa_types::Result<()> {
        if let Some(ids) = &self.thread_ids {
            ids.lock().unwrap().push(thread::current().id());
        }
        if self.fail {
            return Err(VoxaError::new(
                ErrorCategory::Lifecycle,
                "VOXA-TEST-FIRST-ERROR",
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
) -> (voxa_core::GraphRuntime, Arc<Mutex<Vec<u64>>>) {
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
            .start();
    (runtime, received)
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
    .start();
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
    .start();
    match runtime.wait(Duration::from_secs(2)) {
        Err(RuntimeWaitError::Aborted(reason)) => {
            assert_eq!(reason.root().code(), "VOXA-TEST-FIRST-ERROR")
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
    .start();
    runtime.wait(Duration::from_secs(2)).unwrap();
    let observed = ids.lock().unwrap();
    assert!(!observed.is_empty());
    assert!(observed.iter().all(|id| *id != caller));
}
