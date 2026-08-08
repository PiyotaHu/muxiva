use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use muxiva_core::{
    AbortCategory, AbortReason, AbortStage, ConfigSchema, EdgeAction, EdgeContext, EdgeDescriptor,
    EdgePolicies, EdgePolicy, EdgePolicyName, EnabledCondition, GraphBuilder, GraphRunner,
    GraphRunnerBuildError, GraphRunnerState, LifecycleCapabilities, Node, NodeContext,
    NodeDescriptor, NodeInstances, NodeKind, NodeTypeName, PortDescriptor, PortDirection, PortName,
    QueuePolicy, TransformPolicy, ValidationDecision, ValidationFailureAction, ValidationPolicy,
    VisibilityDescriptor,
};
use muxiva_types::{
    ClockDomain, ClockDomainId, ClockKind, EdgeId, ErrorCategory, Extensions, Frame, FrameHeader,
    FrameId, FramePayload, FrameType, Lineage, Metadata, MuxivaError, NamespacedName, NodeId,
    SchemaVersion, SequenceId, SignalData, StreamId, TextData, Timestamp, TraceId, Value,
};

fn assert_send<T: Send>() {}

#[test]
fn runtime_callback_ownership_is_transferable() {
    assert_send::<Box<dyn Node>>();
    assert_send::<Box<dyn EdgePolicy>>();
    assert_send::<NodeInstances>();
    assert_send::<EdgePolicies>();
}

fn node_id(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn port(value: &str) -> PortName {
    PortName::new(value).unwrap()
}

fn edge_id(value: &str) -> EdgeId {
    EdgeId::new(value).unwrap()
}

fn descriptor(
    id: &str,
    kind: NodeKind,
    ports: &[(&str, PortDirection, FrameType)],
) -> NodeDescriptor {
    let node_id = node_id(id);
    NodeDescriptor::new(
        node_id.clone(),
        NodeTypeName::new(format!("test.{id}")).unwrap(),
        kind,
        ports
            .iter()
            .map(|(name, direction, frame_type)| {
                PortDescriptor::new(node_id.clone(), port(name), *direction, *frame_type)
            })
            .collect::<Vec<_>>(),
        ConfigSchema::empty(),
        LifecycleCapabilities::new(true, true, true, true),
    )
}

fn edge(
    id: &str,
    from: &str,
    output: &str,
    to: &str,
    input: &str,
    validation: ValidationPolicy,
    transform: TransformPolicy,
) -> EdgeDescriptor {
    EdgeDescriptor::new(
        edge_id(id),
        node_id(from),
        port(output),
        node_id(to),
        port(input),
        FrameType::Text,
        QueuePolicy::default(),
        validation,
        transform,
        EnabledCondition::Always,
        VisibilityDescriptor::default(),
    )
}

fn named_policy() -> EdgePolicyName {
    EdgePolicyName::new("test.policy").unwrap()
}

fn text_frame(id: &str, text: &str) -> Frame {
    frame(id, FrameType::Text, FramePayload::Text(TextData::new(text)))
}

fn signal_frame(id: &str) -> Frame {
    frame(
        id,
        FrameType::Signal,
        FramePayload::Signal(SignalData::new(
            NamespacedName::new("muxiva.signal.test").unwrap(),
            SchemaVersion::new(1).unwrap(),
            node_id("source"),
            Value::Null,
        )),
    )
}

fn frame(id: &str, frame_type: FrameType, payload: FramePayload) -> Frame {
    let header = FrameHeader::new(
        FrameId::new(id).unwrap(),
        Timestamp::from_nanos(1),
        ClockDomain::new(
            ClockDomainId::new("test.clock").unwrap(),
            ClockKind::MediaRelative,
        ),
        SequenceId::new(1),
        StreamId::new("stream").unwrap(),
        TraceId::new("trace").unwrap(),
        frame_type,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    Frame::new(header, payload).unwrap()
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Hook {
    Prepare,
    Process,
    Finish,
}

enum Behavior {
    Source(Vec<(PortName, Frame)>),
    Uppercase,
    Sink(Arc<Mutex<Vec<Frame>>>),
}

struct TestNode {
    id: &'static str,
    behavior: Behavior,
    log: Arc<Mutex<Vec<String>>>,
    fail: Option<Hook>,
    panic: Option<Hook>,
    abort_panic: bool,
    drops: Option<Arc<AtomicUsize>>,
}

impl TestNode {
    fn new(id: &'static str, behavior: Behavior, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            id,
            behavior,
            log,
            fail: None,
            panic: None,
            abort_panic: false,
            drops: None,
        }
    }

    fn fail(mut self, hook: Hook) -> Self {
        self.fail = Some(hook);
        self
    }

    fn panic(mut self, hook: Hook) -> Self {
        self.panic = Some(hook);
        self
    }

    fn abort_panic(mut self) -> Self {
        self.abort_panic = true;
        self
    }

    fn drop_probe(mut self, drops: Arc<AtomicUsize>) -> Self {
        self.drops = Some(drops);
        self
    }

    fn checkpoint(&self, hook: Hook) -> muxiva_types::Result<()> {
        if self.panic == Some(hook) {
            panic!("{} callback panic", self.id);
        }
        if self.fail == Some(hook) {
            return Err(MuxivaError::new(
                ErrorCategory::Lifecycle,
                "MUXIVA-TEST-NODE",
                format!("{} callback error", self.id),
            ));
        }
        Ok(())
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        if let Some(drops) = &self.drops {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Node for TestNode {
    fn on_prepare(&mut self, context: &mut NodeContext) -> muxiva_types::Result<()> {
        assert_eq!(context.node_id().as_str(), self.id);
        assert!(context.input_port().is_none());
        self.log
            .lock()
            .unwrap()
            .push(format!("prepare:{}", self.id));
        self.checkpoint(Hook::Prepare)
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.log.lock().unwrap().push(format!(
            "process:{}:{}",
            self.id,
            if input.is_some() { "some" } else { "none" }
        ));
        match &mut self.behavior {
            Behavior::Source(outputs) => {
                assert!(input.is_none());
                assert!(context.input_port().is_none());
                for (output, frame) in outputs.iter() {
                    context.emit(output.clone(), frame.clone())?;
                }
            }
            Behavior::Uppercase => {
                let input = input.as_ref().expect("transform input");
                assert_eq!(context.input_port().unwrap().as_str(), "in");
                let value = input.as_text().unwrap().data().as_str().to_uppercase();
                context.emit(port("out"), text_frame("uppercase-frame", &value))?;
            }
            Behavior::Sink(frames) => {
                assert_eq!(context.input_port().unwrap().as_str(), "in");
                frames.lock().unwrap().push(input.expect("sink input"));
            }
        }
        self.checkpoint(Hook::Process)
    }

    fn on_finish(&mut self, context: &mut NodeContext) -> muxiva_types::Result<()> {
        assert!(context.input_port().is_none());
        self.log.lock().unwrap().push(format!("finish:{}", self.id));
        self.checkpoint(Hook::Finish)
    }

    fn on_abort(&mut self, reason: &AbortReason, context: &mut NodeContext) {
        assert!(context.input_port().is_none());
        self.log
            .lock()
            .unwrap()
            .push(format!("abort:{}:{}", self.id, reason.root().code()));
        assert!(!self.abort_panic, "{} abort panic", self.id);
    }
}

#[derive(Clone)]
enum PolicyAction {
    Forward,
    Replace(Frame),
    Drop(&'static str),
    Abort(&'static str),
    Signal(Frame),
}

struct TestPolicy {
    action: PolicyAction,
    validation: ValidationDecision,
    log: Arc<Mutex<Vec<String>>>,
    panic_phase: Option<&'static str>,
    fail_phase: Option<&'static str>,
    drops: Option<Arc<AtomicUsize>>,
}

impl TestPolicy {
    fn new(action: PolicyAction, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            action,
            validation: ValidationDecision::Accept,
            log,
            panic_phase: None,
            fail_phase: None,
            drops: None,
        }
    }

    fn rejecting(mut self, reason: &'static str) -> Self {
        self.validation = ValidationDecision::Reject(reason.into());
        self
    }

    fn panic(mut self, phase: &'static str) -> Self {
        self.panic_phase = Some(phase);
        self
    }

    fn fail(mut self, phase: &'static str) -> Self {
        self.fail_phase = Some(phase);
        self
    }

    fn drop_probe(mut self, drops: Arc<AtomicUsize>) -> Self {
        self.drops = Some(drops);
        self
    }

    fn checkpoint(&self, phase: &'static str) -> muxiva_types::Result<()> {
        assert_ne!(self.panic_phase, Some(phase), "policy {phase} panic");
        if self.fail_phase == Some(phase) {
            return Err(MuxivaError::new(
                ErrorCategory::Lifecycle,
                "MUXIVA-TEST-POLICY",
                format!("policy {phase} error"),
            ));
        }
        Ok(())
    }
}

impl Drop for TestPolicy {
    fn drop(&mut self) {
        if let Some(drops) = &self.drops {
            drops.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl EdgePolicy for TestPolicy {
    fn validate(
        &mut self,
        _frame: &Frame,
        context: &EdgeContext<'_>,
    ) -> muxiva_types::Result<ValidationDecision> {
        assert_eq!(context.descriptor().edge_id().as_str(), "edge");
        assert!(context.graph().edge_count() >= 1);
        assert!(context.graph().node_count() >= 2);
        self.log.lock().unwrap().push("validate".into());
        self.checkpoint("validate")?;
        Ok(self.validation.clone())
    }

    fn transform(
        &mut self,
        frame: &Frame,
        _context: &EdgeContext<'_>,
    ) -> muxiva_types::Result<EdgeAction> {
        self.log.lock().unwrap().push("transform".into());
        self.checkpoint("transform")?;
        Ok(match &self.action {
            PolicyAction::Forward => EdgeAction::Forward(frame.clone()),
            PolicyAction::Replace(frame) => EdgeAction::Replace(frame.clone()),
            PolicyAction::Drop(reason) => EdgeAction::Drop((*reason).into()),
            PolicyAction::Abort(reason) => EdgeAction::Abort((*reason).into()),
            PolicyAction::Signal(frame) => EdgeAction::EmitSignal(frame.clone()),
        })
    }

    fn on_signal(
        &mut self,
        _signal: &muxiva_types::SignalFrame,
        _context: &EdgeContext<'_>,
    ) -> muxiva_types::Result<()> {
        self.log.lock().unwrap().push("signal".into());
        self.checkpoint("on_signal")
    }

    fn on_drop(&mut self, reason: &str, _context: &EdgeContext<'_>) -> muxiva_types::Result<()> {
        self.log.lock().unwrap().push(format!("drop:{reason}"));
        self.checkpoint("on_drop")
    }
}

fn text_pipeline(
    validation: ValidationPolicy,
    transform: TransformPolicy,
) -> muxiva_core::GraphDefinition {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "source",
            NodeKind::Source,
            &[("out", PortDirection::Output, FrameType::Text)],
        ))
        .unwrap()
        .add_node(descriptor(
            "sink",
            NodeKind::Sink,
            &[("in", PortDirection::Input, FrameType::Text)],
        ))
        .unwrap()
        .connect(edge(
            "edge", "source", "out", "sink", "in", validation, transform,
        ))
        .unwrap();
    builder.build().unwrap()
}

fn pipeline_nodes(log: Arc<Mutex<Vec<String>>>, sink: Arc<Mutex<Vec<Frame>>>) -> NodeInstances {
    BTreeMap::from([
        (
            node_id("source"),
            Box::new(TestNode::new(
                "source",
                Behavior::Source(vec![(port("out"), text_frame("original", "hello"))]),
                log.clone(),
            )) as Box<dyn Node>,
        ),
        (
            node_id("sink"),
            Box::new(TestNode::new("sink", Behavior::Sink(sink), log)) as Box<dyn Node>,
        ),
    ])
}

#[test]
fn source_uppercase_sink_runs_complete_deterministic_lifecycle() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(Mutex::new(Vec::new()));
    let mut builder = GraphBuilder::new();
    for node in [
        descriptor(
            "source",
            NodeKind::Source,
            &[("out", PortDirection::Output, FrameType::Text)],
        ),
        descriptor(
            "upper",
            NodeKind::Transform,
            &[
                ("in", PortDirection::Input, FrameType::Text),
                ("out", PortDirection::Output, FrameType::Text),
            ],
        ),
        descriptor(
            "sink",
            NodeKind::Sink,
            &[("in", PortDirection::Input, FrameType::Text)],
        ),
    ] {
        builder.add_node(node).unwrap();
    }
    builder
        .connect(edge(
            "a",
            "source",
            "out",
            "upper",
            "in",
            ValidationPolicy::TypeGateOnly,
            TransformPolicy::Identity,
        ))
        .unwrap()
        .connect(edge(
            "b",
            "upper",
            "out",
            "sink",
            "in",
            ValidationPolicy::TypeGateOnly,
            TransformPolicy::Identity,
        ))
        .unwrap();
    let graph = builder.build().unwrap();
    let nodes = BTreeMap::from([
        (
            node_id("source"),
            Box::new(TestNode::new(
                "source",
                Behavior::Source(vec![(port("out"), text_frame("input", "hello"))]),
                log.clone(),
            )) as Box<dyn Node>,
        ),
        (
            node_id("upper"),
            Box::new(TestNode::new("upper", Behavior::Uppercase, log.clone())) as Box<dyn Node>,
        ),
        (
            node_id("sink"),
            Box::new(TestNode::new(
                "sink",
                Behavior::Sink(sink.clone()),
                log.clone(),
            )) as Box<dyn Node>,
        ),
    ]);
    let mut runner = GraphRunner::new(&graph, nodes, BTreeMap::new()).unwrap();

    runner.run().unwrap();

    assert_eq!(runner.state(), GraphRunnerState::Finished);
    assert_eq!(
        sink.lock().unwrap()[0].as_text().unwrap().data().as_str(),
        "HELLO"
    );
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "prepare:source",
            "prepare:upper",
            "prepare:sink",
            "process:source:none",
            "process:upper:some",
            "process:sink:some",
            "finish:sink",
            "finish:upper",
            "finish:source",
        ]
    );
    for id in ["a", "b"] {
        let metrics = runner.snapshot_edge_metrics(&edge_id(id)).unwrap();
        assert_eq!(metrics.enqueue_total(), 1);
        assert_eq!(metrics.dequeue_total(), 1);
        assert_eq!(metrics.queue_capacity(), 0);
        assert_eq!(metrics.queue_len(), 0);
        assert_eq!(metrics.high_watermark(), 0);
        assert_eq!(metrics.full_total(), 0);
        assert_eq!(metrics.blocked_duration_ns(), 0);
        assert_eq!(metrics.oldest_frame_age_ns(), None);
    }
}

#[test]
fn runtime_maps_are_exact_and_empty_graph_runs_once() {
    let empty = GraphBuilder::new().build().unwrap();
    let mut runner = GraphRunner::new(&empty, BTreeMap::new(), BTreeMap::new()).unwrap();
    assert_eq!(runner.run().unwrap().observed_signal_total(), 0);
    assert_eq!(
        runner.run().unwrap_err().root().code(),
        "MUXIVA-RUN-SINGLE-USE"
    );

    let graph = text_pipeline(ValidationPolicy::TypeGateOnly, TransformPolicy::Identity);
    let missing = GraphRunner::new(&graph, BTreeMap::new(), BTreeMap::new())
        .err()
        .unwrap();
    assert!(matches!(
        missing,
        GraphRunnerBuildError::MissingNodeInstance(id) if id.as_str() == "sink"
    ));

    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(Mutex::new(Vec::new()));
    let mut extra_nodes = pipeline_nodes(log, sink);
    extra_nodes.insert(
        node_id("extra"),
        Box::new(TestNode::new(
            "extra",
            Behavior::Source(Vec::new()),
            Arc::new(Mutex::new(Vec::new())),
        )),
    );
    assert!(matches!(
        GraphRunner::new(&graph, extra_nodes, BTreeMap::new()).err().unwrap(),
        GraphRunnerBuildError::UnknownNodeInstance(id) if id.as_str() == "extra"
    ));

    let named_graph = text_pipeline(
        ValidationPolicy::TypeGateOnly,
        TransformPolicy::Named(named_policy()),
    );
    let missing_policy = GraphRunner::new(
        &named_graph,
        pipeline_nodes(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
        ),
        BTreeMap::new(),
    )
    .err()
    .unwrap();
    assert!(matches!(
        missing_policy,
        GraphRunnerBuildError::MissingEdgePolicy(id) if id.as_str() == "edge"
    ));
}

#[test]
fn policy_order_replace_lineage_and_forward_fanout_are_isolated() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let left = Arc::new(Mutex::new(Vec::new()));
    let right = Arc::new(Mutex::new(Vec::new()));
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "source",
            NodeKind::Source,
            &[("out", PortDirection::Output, FrameType::Text)],
        ))
        .unwrap();
    for id in ["left", "right"] {
        builder
            .add_node(descriptor(
                id,
                NodeKind::Sink,
                &[("in", PortDirection::Input, FrameType::Text)],
            ))
            .unwrap();
    }
    builder
        .connect(edge(
            "edge",
            "source",
            "out",
            "left",
            "in",
            ValidationPolicy::Named {
                policy: named_policy(),
                on_failure: ValidationFailureAction::Drop,
            },
            TransformPolicy::Named(named_policy()),
        ))
        .unwrap()
        .connect(edge(
            "sibling",
            "source",
            "out",
            "right",
            "in",
            ValidationPolicy::TypeGateOnly,
            TransformPolicy::Identity,
        ))
        .unwrap();
    let graph = builder.build().unwrap();
    let nodes = BTreeMap::from([
        (
            node_id("source"),
            Box::new(TestNode::new(
                "source",
                Behavior::Source(vec![(port("out"), text_frame("original", "hello"))]),
                Arc::new(Mutex::new(Vec::new())),
            )) as Box<dyn Node>,
        ),
        (
            node_id("left"),
            Box::new(TestNode::new(
                "left",
                Behavior::Sink(left.clone()),
                Arc::new(Mutex::new(Vec::new())),
            )) as Box<dyn Node>,
        ),
        (
            node_id("right"),
            Box::new(TestNode::new(
                "right",
                Behavior::Sink(right.clone()),
                Arc::new(Mutex::new(Vec::new())),
            )) as Box<dyn Node>,
        ),
    ]);
    let policies = BTreeMap::from([(
        edge_id("edge"),
        Box::new(TestPolicy::new(
            PolicyAction::Replace(text_frame("replacement", "changed")),
            log.clone(),
        )) as Box<dyn EdgePolicy>,
    )]);
    let mut runner = GraphRunner::new(&graph, nodes, policies).unwrap();

    runner.run().unwrap();

    assert_eq!(log.lock().unwrap().as_slice(), ["validate", "transform"]);
    let replacement = left.lock().unwrap()[0].clone();
    assert_eq!(replacement.as_text().unwrap().data().as_str(), "changed");
    assert_eq!(replacement.header().frame_id().as_str(), "replacement");
    assert_eq!(replacement.header().lineage().len(), 1);
    let lineage = replacement.header().lineage().iter().next().unwrap();
    assert_eq!(lineage.parent_frame_id().as_str(), "original");
    assert_eq!(lineage.origin().edge_id().unwrap().as_str(), "edge");
    assert!(lineage.origin().node_id().is_none());
    assert_eq!(
        right.lock().unwrap()[0].as_text().unwrap().data().as_str(),
        "hello"
    );
    assert!(right.lock().unwrap()[0].header().lineage().is_empty());
}

#[test]
fn validation_drop_skips_transform_and_explicit_abort_stops_delivery() {
    for (failure_action, expected_code) in [
        (ValidationFailureAction::Drop, None),
        (
            ValidationFailureAction::Abort,
            Some("MUXIVA-RUN-VALIDATION"),
        ),
    ] {
        let graph = text_pipeline(
            ValidationPolicy::Named {
                policy: named_policy(),
                on_failure: failure_action,
            },
            TransformPolicy::Named(named_policy()),
        );
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::new(Mutex::new(Vec::new()));
        let nodes = pipeline_nodes(Arc::new(Mutex::new(Vec::new())), sink.clone());
        let policies = BTreeMap::from([(
            edge_id("edge"),
            Box::new(TestPolicy::new(PolicyAction::Forward, log.clone()).rejecting("invalid"))
                as Box<dyn EdgePolicy>,
        )]);
        let mut runner = GraphRunner::new(&graph, nodes, policies).unwrap();
        let result = runner.run();

        assert!(sink.lock().unwrap().is_empty());
        if let Some(code) = expected_code {
            assert_eq!(result.unwrap_err().root().code(), code);
            assert_eq!(log.lock().unwrap().as_slice(), ["validate"]);
        } else {
            result.unwrap();
            assert_eq!(log.lock().unwrap().as_slice(), ["validate", "drop:invalid"]);
            let metrics = runner.snapshot_edge_metrics(&edge_id("edge")).unwrap();
            assert_eq!(metrics.drop_total(), 1);
            assert_eq!(metrics.latest_error_reason(), Some("invalid"));
        }
    }
}

#[test]
fn drop_abort_and_emit_signal_actions_have_explicit_stage4_dispositions() {
    for action in [
        PolicyAction::Drop("not wanted"),
        PolicyAction::Abort("stop now"),
        PolicyAction::Signal(signal_frame("signal")),
    ] {
        let graph = text_pipeline(
            ValidationPolicy::TypeGateOnly,
            TransformPolicy::Named(named_policy()),
        );
        let policy_log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::new(Mutex::new(Vec::new()));
        let nodes = pipeline_nodes(Arc::new(Mutex::new(Vec::new())), sink.clone());
        let policies = BTreeMap::from([(
            edge_id("edge"),
            Box::new(TestPolicy::new(action.clone(), policy_log.clone())) as Box<dyn EdgePolicy>,
        )]);
        let mut runner = GraphRunner::new(&graph, nodes, policies).unwrap();
        let result = runner.run();
        let metrics = runner.snapshot_edge_metrics(&edge_id("edge")).unwrap();
        assert!(sink.lock().unwrap().is_empty());

        match action {
            PolicyAction::Drop(_) => {
                result.unwrap();
                assert_eq!(metrics.drop_total(), 1);
                assert_eq!(
                    policy_log.lock().unwrap().as_slice(),
                    ["transform", "drop:not wanted"]
                );
            }
            PolicyAction::Abort(_) => {
                assert_eq!(result.unwrap_err().root().code(), "MUXIVA-RUN-POLICY-ABORT");
                assert_eq!(metrics.latest_error_reason(), Some("stop now"));
            }
            PolicyAction::Signal(_) => {
                assert_eq!(result.unwrap().observed_signal_total(), 1);
                assert_eq!(metrics.signal_total(), 1);
                assert_eq!(runner.observed_signals().len(), 1);
                assert_eq!(
                    policy_log.lock().unwrap().as_slice(),
                    ["transform", "signal"]
                );
            }
            _ => unreachable!(),
        }
    }
}

fn two_source_graph() -> muxiva_core::GraphDefinition {
    let mut builder = GraphBuilder::new();
    for id in ["a", "b"] {
        builder
            .add_node(descriptor(
                id,
                NodeKind::Source,
                &[("out", PortDirection::Output, FrameType::Text)],
            ))
            .unwrap();
    }
    builder.build().unwrap()
}

fn failure_nodes(
    log: Arc<Mutex<Vec<String>>>,
    failing: &'static str,
    hook: Hook,
    panic: bool,
    abort_panic: bool,
) -> NodeInstances {
    ["a", "b"]
        .into_iter()
        .map(|id| {
            let mut node = TestNode::new(id, Behavior::Source(Vec::new()), log.clone());
            if id == failing {
                node = if panic {
                    node.panic(hook)
                } else {
                    node.fail(hook)
                };
            }
            if id == "b" && abort_panic {
                node = node.abort_panic();
            }
            (node_id(id), Box::new(node) as Box<dyn Node>)
        })
        .collect()
}

#[test]
fn prepare_process_finish_errors_abort_prepared_nodes_once_in_reverse_order() {
    let graph = two_source_graph();
    for (hook, failing, expected_stage, expected_tail) in [
        (
            Hook::Prepare,
            "b",
            AbortStage::Prepare,
            vec!["abort:a:MUXIVA-TEST-NODE"],
        ),
        (
            Hook::Process,
            "a",
            AbortStage::Process,
            vec!["abort:b:MUXIVA-TEST-NODE", "abort:a:MUXIVA-TEST-NODE"],
        ),
        (
            Hook::Finish,
            "b",
            AbortStage::Finish,
            vec!["abort:b:MUXIVA-TEST-NODE", "abort:a:MUXIVA-TEST-NODE"],
        ),
    ] {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut runner = GraphRunner::new(
            &graph,
            failure_nodes(log.clone(), failing, hook, false, false),
            BTreeMap::new(),
        )
        .unwrap();
        let reason = runner.run().unwrap_err();
        assert_eq!(reason.category(), AbortCategory::NodeError);
        assert_eq!(reason.stage(), expected_stage);
        assert_eq!(reason.node_id().unwrap().as_str(), failing);
        let events = log.lock().unwrap();
        assert!(events.ends_with(
            &expected_tail
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        ));
        for id in ["a", "b"] {
            assert!(
                events
                    .iter()
                    .filter(|event| event.starts_with(&format!("abort:{id}:")))
                    .count()
                    <= 1
            );
        }
    }
}

#[test]
fn node_and_policy_panics_are_caught_and_abort_hook_panics_do_not_stop_cleanup() {
    let graph = two_source_graph();
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runner = GraphRunner::new(
        &graph,
        failure_nodes(log.clone(), "a", Hook::Process, true, true),
        BTreeMap::new(),
    )
    .unwrap();
    let reason = runner.run().unwrap_err();
    assert_eq!(reason.category(), AbortCategory::RustPanic);
    assert_eq!(reason.stage(), AbortStage::Process);
    assert_eq!(runner.abort_diagnostics().len(), 1);
    assert_eq!(runner.abort_diagnostics()[0].node_id().as_str(), "b");
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|event| event.starts_with("abort:a:")));

    let graph = text_pipeline(
        ValidationPolicy::TypeGateOnly,
        TransformPolicy::Named(named_policy()),
    );
    let sink = Arc::new(Mutex::new(Vec::new()));
    let nodes = pipeline_nodes(Arc::new(Mutex::new(Vec::new())), sink);
    let policies = BTreeMap::from([(
        edge_id("edge"),
        Box::new(
            TestPolicy::new(PolicyAction::Forward, Arc::new(Mutex::new(Vec::new())))
                .panic("transform"),
        ) as Box<dyn EdgePolicy>,
    )]);
    let mut runner = GraphRunner::new(&graph, nodes, policies).unwrap();
    let reason = runner.run().unwrap_err();
    assert_eq!(reason.category(), AbortCategory::RustPanic);
    assert_eq!(reason.stage(), AbortStage::Runtime);
    assert_eq!(
        reason.root().details().get("edge_id"),
        Some(&Value::String("edge".into()))
    );
}

#[test]
fn policy_hook_errors_and_panics_are_terminal_and_resource_owners_are_released() {
    for (phase, panic) in [("validate", false), ("on_drop", true)] {
        let validation = ValidationPolicy::Named {
            policy: named_policy(),
            on_failure: ValidationFailureAction::Drop,
        };
        let graph = text_pipeline(validation, TransformPolicy::Identity);
        let node_drops = Arc::new(AtomicUsize::new(0));
        let policy_drops = Arc::new(AtomicUsize::new(0));
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::new(Mutex::new(Vec::new()));
        let mut nodes = pipeline_nodes(log, sink);
        for node in nodes.values_mut() {
            // Replace the map below with probed nodes because trait objects cannot be downcast.
            let _ = node;
        }
        nodes = BTreeMap::from([
            (
                node_id("source"),
                Box::new(
                    TestNode::new(
                        "source",
                        Behavior::Source(vec![(port("out"), text_frame("original", "hello"))]),
                        Arc::new(Mutex::new(Vec::new())),
                    )
                    .drop_probe(node_drops.clone()),
                ) as Box<dyn Node>,
            ),
            (
                node_id("sink"),
                Box::new(
                    TestNode::new(
                        "sink",
                        Behavior::Sink(Arc::new(Mutex::new(Vec::new()))),
                        Arc::new(Mutex::new(Vec::new())),
                    )
                    .drop_probe(node_drops.clone()),
                ) as Box<dyn Node>,
            ),
        ]);
        let mut policy = TestPolicy::new(PolicyAction::Forward, Arc::new(Mutex::new(Vec::new())))
            .rejecting("drop me")
            .drop_probe(policy_drops.clone());
        policy = if panic {
            policy.panic(phase)
        } else {
            policy.fail(phase)
        };
        let policies = BTreeMap::from([(edge_id("edge"), Box::new(policy) as Box<dyn EdgePolicy>)]);
        let mut runner = GraphRunner::new(&graph, nodes, policies).unwrap();

        let reason = runner.run().unwrap_err();

        assert_eq!(
            reason.category(),
            if panic {
                AbortCategory::RustPanic
            } else {
                AbortCategory::NodeError
            }
        );
        assert_eq!(node_drops.load(Ordering::Relaxed), 2);
        assert_eq!(policy_drops.load(Ordering::Relaxed), 1);
    }
}
