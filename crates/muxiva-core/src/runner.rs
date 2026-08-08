use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    fmt,
    panic::{catch_unwind, AssertUnwindSafe},
    time::Duration,
};

use muxiva_types::{EdgeId, ErrorCategory, Frame, MuxivaError, NodeId, TransformOrigin, Value};

use crate::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigKey, ConfigMap, EdgeAction,
    EdgeContext, EdgeDescriptor, EdgeMetricsSnapshot, EdgePolicy, EnabledCondition,
    GraphDefinition, Node, NodeContext, NodeKind, PortDirection, PortName, TransformPolicy,
    ValidationDecision, ValidationFailureAction, ValidationPolicy,
};

/// Runtime node implementations keyed separately from stable graph data.
pub type NodeInstances = BTreeMap<NodeId, Box<dyn Node>>;

/// Per-Edge policy implementations for enabled Edges with named behavior.
pub type EdgePolicies = BTreeMap<EdgeId, Box<dyn EdgePolicy>>;

/// The externally observable state of a single-use synchronous runner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphRunnerState {
    /// Runtime consistency checks passed; no user callback has run.
    Ready,
    /// Node preparation is in progress.
    Preparing,
    /// Source and downstream processing is in progress.
    Running,
    /// Reverse-topological normal completion is in progress.
    Finishing,
    /// Reverse-topological failure cleanup is in progress.
    Aborting,
    /// The graph completed successfully.
    Finished,
    /// The graph stopped after its first terminal failure.
    Aborted,
}

/// A Stage 4 policy signal observation retained without adjacent node routing.
#[derive(Clone, Eq, PartialEq)]
pub struct ObservedEdgeSignal {
    edge_id: EdgeId,
    frame: Frame,
}

impl ObservedEdgeSignal {
    /// Returns the Edge on which the signal was emitted.
    pub fn edge_id(&self) -> &EdgeId {
        &self.edge_id
    }

    /// Returns the concrete immutable Signal frame.
    pub const fn frame(&self) -> &Frame {
        &self.frame
    }
}

/// Bounded diagnostic information from a panicking abort hook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbortHookDiagnostic {
    node_id: NodeId,
    message: Box<str>,
}

impl AbortHookDiagnostic {
    pub(crate) fn new(node_id: NodeId, message: Box<str>) -> Self {
        Self { node_id, message }
    }

    /// Returns the aborting node whose cleanup hook panicked.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns a bounded diagnostic message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Summary of one successful synchronous run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GraphRunSummary {
    observed_signal_total: usize,
}

impl GraphRunSummary {
    /// Returns the number of Stage 4 signal observations.
    pub const fn observed_signal_total(self) -> usize {
        self.observed_signal_total
    }
}

/// Runtime-map or declarative-condition inconsistency detected before callbacks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphRunnerBuildError {
    /// A graph node has no runtime implementation.
    MissingNodeInstance(NodeId),
    /// A runtime implementation has no corresponding graph node.
    UnknownNodeInstance(NodeId),
    /// An enabled named Edge has no runtime policy.
    MissingEdgePolicy(EdgeId),
    /// A runtime policy is not required by an enabled named Edge.
    UnknownEdgePolicy(EdgeId),
    /// A configuration condition refers to a node absent from the definition.
    InvalidEnabledCondition { edge_id: EdgeId, node_id: NodeId },
}

impl fmt::Display for GraphRunnerBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingNodeInstance(node_id) => {
                write!(formatter, "missing runtime instance for node `{node_id}`")
            }
            Self::UnknownNodeInstance(node_id) => {
                write!(
                    formatter,
                    "runtime instance targets unknown node `{node_id}`"
                )
            }
            Self::MissingEdgePolicy(edge_id) => {
                write!(
                    formatter,
                    "missing runtime policy for enabled edge `{edge_id}`"
                )
            }
            Self::UnknownEdgePolicy(edge_id) => {
                write!(
                    formatter,
                    "runtime policy targets non-named or disabled edge `{edge_id}`"
                )
            }
            Self::InvalidEnabledCondition { edge_id, node_id } => write!(
                formatter,
                "edge `{edge_id}` enabled condition targets missing node `{node_id}`"
            ),
        }
    }
}

impl Error for GraphRunnerBuildError {}

#[derive(Clone)]
struct DispatchItem {
    node_id: NodeId,
    input_port: PortName,
    frame: Frame,
}

/// Deterministic, single-threaded and single-use graph executor.
pub struct GraphRunner<'graph> {
    graph: &'graph GraphDefinition,
    nodes: NodeInstances,
    policies: EdgePolicies,
    routes: BTreeMap<(NodeId, PortName), Box<[EdgeId]>>,
    enabled_edges: BTreeSet<EdgeId>,
    metrics: BTreeMap<EdgeId, EdgeMetricsSnapshot>,
    state: GraphRunnerState,
    prepared: BTreeSet<NodeId>,
    aborted: BTreeSet<NodeId>,
    observed_signals: Vec<ObservedEdgeSignal>,
    abort_diagnostics: Vec<AbortHookDiagnostic>,
    notification_bus: crate::NotificationBus,
    resources: crate::ResourceStore,
}

impl<'graph> GraphRunner<'graph> {
    /// Compiles a stable routing plan and verifies all runtime attachments.
    pub fn new(
        graph: &'graph GraphDefinition,
        nodes: NodeInstances,
        policies: EdgePolicies,
    ) -> Result<Self, GraphRunnerBuildError> {
        validate_node_instances(graph, &nodes)?;

        let enabled_edges = enabled_edges(graph)?;
        let expected_policies = graph
            .edges()
            .iter()
            .filter(|edge| enabled_edges.contains(edge.edge_id()) && edge_uses_named_policy(edge))
            .map(|edge| edge.edge_id().clone())
            .collect::<BTreeSet<_>>();
        validate_edge_policies(&expected_policies, &policies)?;

        let mut routes = BTreeMap::<(NodeId, PortName), Vec<EdgeId>>::new();
        for edge in graph.edges() {
            routes
                .entry((edge.from_node_id().clone(), edge.from_output_port().clone()))
                .or_default()
                .push(edge.edge_id().clone());
        }
        let routes = routes
            .into_iter()
            .map(|(endpoint, edges)| (endpoint, edges.into_boxed_slice()))
            .collect();
        let metrics = graph
            .edges()
            .iter()
            .map(|edge| {
                // Stage 4 has no queue; every queue/backpressure field is neutral.
                (
                    edge.edge_id().clone(),
                    EdgeMetricsSnapshot::zero(edge.edge_id().clone(), 0),
                )
            })
            .collect();

        Ok(Self {
            graph,
            nodes,
            policies,
            routes,
            enabled_edges,
            metrics,
            state: GraphRunnerState::Ready,
            prepared: BTreeSet::new(),
            aborted: BTreeSet::new(),
            observed_signals: Vec::new(),
            abort_diagnostics: Vec::new(),
            notification_bus: crate::NotificationBus::default(),
            resources: crate::ResourceStore::new(),
        })
    }

    /// Replaces the runtime-wide NotificationBus exposed through every NodeContext.
    pub fn with_notification_bus(mut self, notification_bus: crate::NotificationBus) -> Self {
        self.notification_bus = notification_bus;
        self
    }

    /// Replaces the graph-local resources exposed through every NodeContext.
    pub fn with_resources(mut self, resources: crate::ResourceStore) -> Self {
        self.resources = resources;
        self
    }

    /// Runs the complete graph lifecycle once on the calling thread.
    pub fn run(&mut self) -> Result<GraphRunSummary, AbortReason> {
        if self.state != GraphRunnerState::Ready {
            return Err(runtime_abort(
                "MUXIVA-RUN-SINGLE-USE",
                "a GraphRunner can execute only once",
                None,
                AbortStage::Runtime,
            ));
        }

        match self.execute() {
            Ok(()) => {
                self.state = GraphRunnerState::Finished;
                self.release_runtime_resources();
                Ok(GraphRunSummary {
                    observed_signal_total: self.observed_signals.len(),
                })
            }
            Err(reason) => {
                self.state = GraphRunnerState::Aborting;
                self.abort_all_prepared(&reason);
                self.state = GraphRunnerState::Aborted;
                self.release_runtime_resources();
                Err(reason)
            }
        }
    }

    /// Returns the current lifecycle state.
    pub const fn state(&self) -> GraphRunnerState {
        self.state
    }

    /// Returns a coherent metrics snapshot for one declared Edge.
    pub fn snapshot_edge_metrics(&self, edge_id: &EdgeId) -> Option<EdgeMetricsSnapshot> {
        self.metrics.get(edge_id).cloned()
    }

    /// Returns Stage 4 signal observations in deterministic action order.
    pub fn observed_signals(&self) -> &[ObservedEdgeSignal] {
        &self.observed_signals
    }

    /// Returns cleanup-hook panic diagnostics without replacing the first error.
    pub fn abort_diagnostics(&self) -> &[AbortHookDiagnostic] {
        &self.abort_diagnostics
    }

    fn execute(&mut self) -> Result<(), AbortReason> {
        self.state = GraphRunnerState::Preparing;
        for node_id in self.graph.topological_order() {
            self.call_prepare(node_id)?;
            self.prepared.insert(node_id.clone());
        }

        self.state = GraphRunnerState::Running;
        for node_id in self.graph.topological_order() {
            let definition = self
                .graph
                .node(node_id)
                .expect("topology contains only graph nodes");
            if definition.descriptor().kind() != NodeKind::Source {
                continue;
            }

            let emissions = self.call_process(node_id, None, None)?;
            let mut worklist = VecDeque::new();
            self.route_emissions(node_id, emissions, &mut worklist)?;
            while let Some(item) = worklist.pop_front() {
                let emissions =
                    self.call_process(&item.node_id, Some(item.frame), Some(item.input_port))?;
                self.route_emissions(&item.node_id, emissions, &mut worklist)?;
            }
        }

        self.state = GraphRunnerState::Finishing;
        for node_id in self.graph.topological_order().iter().rev() {
            self.call_finish(node_id)?;
        }
        Ok(())
    }

    fn call_prepare(&mut self, node_id: &NodeId) -> Result<(), AbortReason> {
        let config = self
            .graph
            .node(node_id)
            .expect("validated node")
            .config()
            .clone();
        let mut context = NodeContext::with_runtime_services(
            node_id.clone(),
            config,
            None,
            self.notification_bus.clone(),
            self.resources.clone(),
        );
        let node = self.nodes.get_mut(node_id).expect("validated instance");
        match catch_unwind(AssertUnwindSafe(|| node.on_prepare(&mut context))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(node_error(error, node_id, AbortStage::Prepare)),
            Err(payload) => Err(panic_abort(
                payload,
                Some(node_id),
                AbortStage::Prepare,
                None,
            )),
        }
    }

    fn call_process(
        &mut self,
        node_id: &NodeId,
        input: Option<Frame>,
        input_port: Option<PortName>,
    ) -> Result<Vec<crate::NodeEmission>, AbortReason> {
        let config = self
            .graph
            .node(node_id)
            .expect("validated node")
            .config()
            .clone();
        let mut context = NodeContext::with_runtime_services(
            node_id.clone(),
            config,
            input_port,
            self.notification_bus.clone(),
            self.resources.clone(),
        );
        let node = self.nodes.get_mut(node_id).expect("validated instance");
        match catch_unwind(AssertUnwindSafe(|| node.on_process(input, &mut context))) {
            Ok(Ok(())) => Ok(context.take_emissions()),
            Ok(Err(error)) => Err(node_error(error, node_id, AbortStage::Process)),
            Err(payload) => Err(panic_abort(
                payload,
                Some(node_id),
                AbortStage::Process,
                None,
            )),
        }
    }

    fn call_finish(&mut self, node_id: &NodeId) -> Result<(), AbortReason> {
        let config = self
            .graph
            .node(node_id)
            .expect("validated node")
            .config()
            .clone();
        let mut context = NodeContext::with_runtime_services(
            node_id.clone(),
            config,
            None,
            self.notification_bus.clone(),
            self.resources.clone(),
        );
        let node = self.nodes.get_mut(node_id).expect("validated instance");
        match catch_unwind(AssertUnwindSafe(|| node.on_finish(&mut context))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(node_error(error, node_id, AbortStage::Finish)),
            Err(payload) => Err(panic_abort(
                payload,
                Some(node_id),
                AbortStage::Finish,
                None,
            )),
        }
    }

    fn route_emissions(
        &mut self,
        node_id: &NodeId,
        emissions: Vec<crate::NodeEmission>,
        worklist: &mut VecDeque<DispatchItem>,
    ) -> Result<(), AbortReason> {
        let descriptor = self
            .graph
            .node(node_id)
            .expect("validated node")
            .descriptor();
        for emission in emissions {
            let (output_port, frame) = emission.into_parts();
            let Some(port) = descriptor.ports().iter().find(|port| {
                port.name() == &output_port && port.direction() == PortDirection::Output
            }) else {
                return Err(runtime_abort_with_details(
                    "MUXIVA-RUN-OUTPUT-PORT",
                    "node emitted through an undeclared output port",
                    Some(node_id.clone()),
                    AbortStage::Process,
                    [("output_port", output_port.as_str())],
                ));
            };
            if frame.frame_type() != port.frame_type() {
                return Err(runtime_abort_with_details(
                    "MUXIVA-RUN-OUTPUT-TYPE",
                    "node emitted a frame whose type does not match its output port",
                    Some(node_id.clone()),
                    AbortStage::Process,
                    [
                        ("output_port", output_port.as_str()),
                        ("expected_type", frame_type_name(port.frame_type())),
                        ("actual_type", frame_type_name(frame.frame_type())),
                    ],
                ));
            }

            let edge_ids = self
                .routes
                .get(&(node_id.clone(), output_port))
                .cloned()
                .unwrap_or_default();
            for edge_id in edge_ids {
                if self.enabled_edges.contains(&edge_id) {
                    self.apply_edge(&edge_id, frame.clone(), worklist)?;
                }
            }
        }
        Ok(())
    }

    fn apply_edge(
        &mut self,
        edge_id: &EdgeId,
        original: Frame,
        worklist: &mut VecDeque<DispatchItem>,
    ) -> Result<(), AbortReason> {
        let edge = self.graph.edge(edge_id).expect("compiled edge");
        if original.frame_type() != edge.frame_type() {
            return self.edge_error(
                edge,
                "MUXIVA-RUN-EDGE-TYPE",
                "frame failed the mandatory exact Edge type gate",
            );
        }

        let validation = match edge.validation_policy() {
            ValidationPolicy::TypeGateOnly => ValidationDecision::Accept,
            ValidationPolicy::Named { .. } => {
                let snapshot = self.metrics.get(edge_id).expect("metrics exist").clone();
                let context = EdgeContext::new(self.graph, edge, &snapshot);
                let policy = self.policies.get_mut(edge_id).expect("validated policy");
                match catch_unwind(AssertUnwindSafe(|| policy.validate(&original, &context))) {
                    Ok(Ok(decision)) => decision,
                    Ok(Err(error)) => return self.policy_returned_error(edge, error, "validate"),
                    Err(payload) => return self.policy_panicked(edge, payload, "validate"),
                }
            }
        };

        if let ValidationDecision::Reject(reason) = validation {
            return match edge.validation_policy() {
                ValidationPolicy::Named {
                    on_failure: ValidationFailureAction::Abort,
                    ..
                } => {
                    self.metrics
                        .get_mut(edge_id)
                        .expect("metrics exist")
                        .record_error(&reason);
                    Err(policy_abort(
                        edge,
                        "MUXIVA-RUN-VALIDATION",
                        &reason,
                        "validate",
                    ))
                }
                ValidationPolicy::Named {
                    on_failure: ValidationFailureAction::Drop,
                    ..
                }
                | ValidationPolicy::TypeGateOnly => self.drop_frame(edge, &reason),
            };
        }

        let action = match edge.transform_policy() {
            TransformPolicy::Identity => EdgeAction::Forward(original.clone()),
            TransformPolicy::Named(_) => {
                let snapshot = self.metrics.get(edge_id).expect("metrics exist").clone();
                let context = EdgeContext::new(self.graph, edge, &snapshot);
                let policy = self.policies.get_mut(edge_id).expect("validated policy");
                match catch_unwind(AssertUnwindSafe(|| policy.transform(&original, &context))) {
                    Ok(Ok(action)) => action,
                    Ok(Err(error)) => return self.policy_returned_error(edge, error, "transform"),
                    Err(payload) => return self.policy_panicked(edge, payload, "transform"),
                }
            }
        };

        match action {
            EdgeAction::Forward(frame) => {
                if frame != original || frame.header().frame_id() != original.header().frame_id() {
                    return self.edge_error(
                        edge,
                        "MUXIVA-RUN-FORWARD-MUTATION",
                        "Forward must return the unchanged input frame; use Replace",
                    );
                }
                self.deliver(edge, frame, worklist)
            }
            EdgeAction::Replace(replacement) => {
                if replacement.header().frame_id() == original.header().frame_id() {
                    return self.edge_error(
                        edge,
                        "MUXIVA-RUN-REPLACE-ID",
                        "Replace must return a frame with a fresh ID",
                    );
                }
                if replacement.frame_type() != edge.frame_type() {
                    return self.edge_error(
                        edge,
                        "MUXIVA-RUN-REPLACE-TYPE",
                        "replacement frame type does not match the Edge",
                    );
                }
                let origin = TransformOrigin::new(None, Some(edge_id.clone()))
                    .map_err(|error| node_error(error, edge.from_node_id(), AbortStage::Runtime))?;
                let replacement = original
                    .attach_replacement_lineage(replacement, origin, "edge policy replacement")
                    .map_err(|error| node_error(error, edge.from_node_id(), AbortStage::Runtime))?;
                self.deliver(edge, replacement, worklist)
            }
            EdgeAction::Drop(reason) => self.drop_frame(edge, &reason),
            EdgeAction::Abort(reason) => {
                self.metrics
                    .get_mut(edge_id)
                    .expect("metrics exist")
                    .record_error(&reason);
                Err(policy_abort(
                    edge,
                    "MUXIVA-RUN-POLICY-ABORT",
                    &reason,
                    "transform",
                ))
            }
            EdgeAction::EmitSignal(frame) => {
                if frame.frame_type() != muxiva_types::FrameType::Signal {
                    return self.edge_error(
                        edge,
                        "MUXIVA-RUN-SIGNAL-TYPE",
                        "EmitSignal requires a Signal frame",
                    );
                }
                self.metrics
                    .get_mut(edge_id)
                    .expect("metrics exist")
                    .record_signal();
                let snapshot = self.metrics.get(edge_id).expect("metrics exist").clone();
                let context = EdgeContext::new(self.graph, edge, &snapshot);
                let signal = frame.as_signal().expect("type checked");
                let policy = self
                    .policies
                    .get_mut(edge_id)
                    .expect("named transform policy");
                match catch_unwind(AssertUnwindSafe(|| policy.on_signal(signal, &context))) {
                    Ok(Ok(())) => {
                        self.observed_signals.push(ObservedEdgeSignal {
                            edge_id: edge_id.clone(),
                            frame,
                        });
                        Ok(())
                    }
                    Ok(Err(error)) => self.policy_returned_error(edge, error, "on_signal"),
                    Err(payload) => self.policy_panicked(edge, payload, "on_signal"),
                }
            }
        }
    }

    fn deliver(
        &mut self,
        edge: &EdgeDescriptor,
        frame: Frame,
        worklist: &mut VecDeque<DispatchItem>,
    ) -> Result<(), AbortReason> {
        if frame.frame_type() != edge.frame_type() {
            return self.edge_error(
                edge,
                "MUXIVA-RUN-DELIVERY-TYPE",
                "policy output type does not match the Edge destination",
            );
        }
        self.metrics
            .get_mut(edge.edge_id())
            .expect("metrics exist")
            .record_delivery();
        worklist.push_back(DispatchItem {
            node_id: edge.to_node_id().clone(),
            input_port: edge.to_input_port().clone(),
            frame,
        });
        Ok(())
    }

    fn drop_frame(&mut self, edge: &EdgeDescriptor, reason: &str) -> Result<(), AbortReason> {
        self.metrics
            .get_mut(edge.edge_id())
            .expect("metrics exist")
            .record_drop(reason);
        let snapshot = self
            .metrics
            .get(edge.edge_id())
            .expect("metrics exist")
            .clone();
        let context = EdgeContext::new(self.graph, edge, &snapshot);
        let policy = self
            .policies
            .get_mut(edge.edge_id())
            .expect("drops originate from a named policy");
        match catch_unwind(AssertUnwindSafe(|| policy.on_drop(reason, &context))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => self.policy_returned_error(edge, error, "on_drop"),
            Err(payload) => self.policy_panicked(edge, payload, "on_drop"),
        }
    }

    fn edge_error<T>(
        &mut self,
        edge: &EdgeDescriptor,
        code: &'static str,
        message: &'static str,
    ) -> Result<T, AbortReason> {
        self.metrics
            .get_mut(edge.edge_id())
            .expect("metrics exist")
            .record_error(message);
        Err(policy_abort(edge, code, message, "runtime"))
    }

    fn policy_returned_error<T>(
        &mut self,
        edge: &EdgeDescriptor,
        error: MuxivaError,
        phase: &'static str,
    ) -> Result<T, AbortReason> {
        self.metrics
            .get_mut(edge.edge_id())
            .expect("metrics exist")
            .record_error(error.message());
        let category = abort_category(error.category());
        Err(AbortReason::new(
            category,
            Some(edge.from_node_id().clone()),
            AbortStage::Runtime,
            AbortRootContext::new(
                Box::<str>::from(error.code()),
                bounded_message(error.message()),
                details([
                    ("edge_id", edge.edge_id().as_str()),
                    ("policy_phase", phase),
                ]),
            ),
        ))
    }

    fn policy_panicked<T>(
        &mut self,
        edge: &EdgeDescriptor,
        payload: Box<dyn Any + Send>,
        phase: &'static str,
    ) -> Result<T, AbortReason> {
        let message = panic_message(payload.as_ref());
        self.metrics
            .get_mut(edge.edge_id())
            .expect("metrics exist")
            .record_error(&message);
        Err(panic_abort(
            payload,
            Some(edge.from_node_id()),
            AbortStage::Runtime,
            Some((edge.edge_id(), phase)),
        ))
    }

    fn abort_all_prepared(&mut self, reason: &AbortReason) {
        for node_id in self.graph.topological_order().iter().rev() {
            if !self.prepared.contains(node_id) || !self.aborted.insert(node_id.clone()) {
                continue;
            }
            let config = self
                .graph
                .node(node_id)
                .expect("validated node")
                .config()
                .clone();
            let mut context = NodeContext::with_runtime_services(
                node_id.clone(),
                config,
                None,
                self.notification_bus.clone(),
                self.resources.clone(),
            );
            let node = self.nodes.get_mut(node_id).expect("prepared instance");
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
                node.on_abort(reason, &mut context);
            })) {
                self.abort_diagnostics.push(AbortHookDiagnostic {
                    node_id: node_id.clone(),
                    message: panic_message(payload.as_ref()),
                });
            }
        }
    }

    fn release_runtime_resources(&mut self) {
        self.nodes.clear();
        self.policies.clear();
        let _ = self.notification_bus.stop(Duration::from_millis(100));
        self.resources.stop();
    }
}

fn validate_node_instances(
    graph: &GraphDefinition,
    nodes: &NodeInstances,
) -> Result<(), GraphRunnerBuildError> {
    let expected = graph
        .nodes()
        .iter()
        .map(|node| node.descriptor().node_id().clone())
        .collect::<BTreeSet<_>>();
    let actual = nodes.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(node_id) = expected.difference(&actual).next() {
        return Err(GraphRunnerBuildError::MissingNodeInstance(node_id.clone()));
    }
    if let Some(node_id) = actual.difference(&expected).next() {
        return Err(GraphRunnerBuildError::UnknownNodeInstance(node_id.clone()));
    }
    Ok(())
}

fn validate_edge_policies(
    expected: &BTreeSet<EdgeId>,
    policies: &EdgePolicies,
) -> Result<(), GraphRunnerBuildError> {
    let actual = policies.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(edge_id) = expected.difference(&actual).next() {
        return Err(GraphRunnerBuildError::MissingEdgePolicy(edge_id.clone()));
    }
    if let Some(edge_id) = actual.difference(expected).next() {
        return Err(GraphRunnerBuildError::UnknownEdgePolicy(edge_id.clone()));
    }
    Ok(())
}

fn enabled_edges(graph: &GraphDefinition) -> Result<BTreeSet<EdgeId>, GraphRunnerBuildError> {
    let mut enabled = BTreeSet::new();
    for edge in graph.edges() {
        let is_enabled = match edge.enabled() {
            EnabledCondition::Always => true,
            EnabledCondition::Never => false,
            EnabledCondition::ConfigEquals {
                node_id,
                key,
                expected,
            } => {
                let node = graph.node(node_id).ok_or_else(|| {
                    GraphRunnerBuildError::InvalidEnabledCondition {
                        edge_id: edge.edge_id().clone(),
                        node_id: node_id.clone(),
                    }
                })?;
                node.config().get(key.as_str()) == Some(expected)
            }
        };
        if is_enabled {
            enabled.insert(edge.edge_id().clone());
        }
    }
    Ok(enabled)
}

fn edge_uses_named_policy(edge: &EdgeDescriptor) -> bool {
    matches!(edge.validation_policy(), ValidationPolicy::Named { .. })
        || matches!(edge.transform_policy(), TransformPolicy::Named(_))
}

fn node_error(error: MuxivaError, node_id: &NodeId, stage: AbortStage) -> AbortReason {
    AbortReason::new(
        abort_category(error.category()),
        Some(node_id.clone()),
        stage,
        AbortRootContext::new(
            Box::<str>::from(error.code()),
            bounded_message(error.message()),
            ConfigMap::empty(),
        ),
    )
}

fn abort_category(category: ErrorCategory) -> AbortCategory {
    match category {
        ErrorCategory::Cancelled => AbortCategory::Cancelled,
        ErrorCategory::External => AbortCategory::ExternalSdkError,
        ErrorCategory::Configuration
        | ErrorCategory::Validation
        | ErrorCategory::Lifecycle
        | ErrorCategory::Internal => AbortCategory::NodeError,
    }
}

fn runtime_abort(
    code: &'static str,
    message: &'static str,
    node_id: Option<NodeId>,
    stage: AbortStage,
) -> AbortReason {
    AbortReason::new(
        AbortCategory::NodeError,
        node_id,
        stage,
        AbortRootContext::new(code, message, ConfigMap::empty()),
    )
}

fn runtime_abort_with_details<const N: usize>(
    code: &'static str,
    message: &'static str,
    node_id: Option<NodeId>,
    stage: AbortStage,
    context: [(&str, &str); N],
) -> AbortReason {
    AbortReason::new(
        AbortCategory::NodeError,
        node_id,
        stage,
        AbortRootContext::new(code, message, details(context)),
    )
}

fn policy_abort(
    edge: &EdgeDescriptor,
    code: &'static str,
    message: &str,
    phase: &'static str,
) -> AbortReason {
    AbortReason::new(
        AbortCategory::NodeError,
        Some(edge.from_node_id().clone()),
        AbortStage::Runtime,
        AbortRootContext::new(
            code,
            bounded_message(message),
            details([
                ("edge_id", edge.edge_id().as_str()),
                ("policy_phase", phase),
            ]),
        ),
    )
}

fn panic_abort(
    payload: Box<dyn Any + Send>,
    node_id: Option<&NodeId>,
    stage: AbortStage,
    edge: Option<(&EdgeId, &'static str)>,
) -> AbortReason {
    let details = edge.map_or_else(ConfigMap::empty, |(edge_id, phase)| {
        details([("edge_id", edge_id.as_str()), ("policy_phase", phase)])
    });
    AbortReason::new(
        AbortCategory::RustPanic,
        node_id.cloned(),
        stage,
        AbortRootContext::new("MUXIVA-RUN-PANIC", panic_message(payload.as_ref()), details),
    )
}

fn panic_message(payload: &(dyn Any + Send)) -> Box<str> {
    let message = if let Some(message) = payload.downcast_ref::<&str>() {
        *message
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.as_str()
    } else {
        "Rust task panicked with a non-string payload"
    };
    bounded_message(message)
}

fn bounded_message(message: &str) -> Box<str> {
    const MAX_MESSAGE_BYTES: usize = 256;
    let mut sanitized = String::new();
    for character in message.chars() {
        let character = if character.is_ascii_control() {
            ' '
        } else {
            character
        };
        if sanitized.len() + character.len_utf8() > MAX_MESSAGE_BYTES {
            break;
        }
        sanitized.push(character);
    }
    sanitized.into_boxed_str()
}

fn details<const N: usize>(values: [(&str, &str); N]) -> ConfigMap {
    ConfigMap::try_from_iter(values.map(|(key, value)| {
        (
            ConfigKey::new(key).expect("static detail key is valid"),
            Value::String(Box::from(value)),
        )
    }))
    .expect("static detail keys are unique")
}

const fn frame_type_name(frame_type: muxiva_types::FrameType) -> &'static str {
    match frame_type {
        muxiva_types::FrameType::Audio => "Audio",
        muxiva_types::FrameType::Video => "Video",
        muxiva_types::FrameType::Text => "Text",
        muxiva_types::FrameType::Byte => "Byte",
        muxiva_types::FrameType::Signal => "Signal",
        muxiva_types::FrameType::Event => "Event",
    }
}
