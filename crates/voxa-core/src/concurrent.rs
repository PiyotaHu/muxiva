use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc, Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

use voxa_types::{EdgeId, ErrorCategory, Frame, NodeId, TransformOrigin, Value, VoxaError};

use crate::queue::QueueWake;
use crate::{
    AbortCategory, AbortHookDiagnostic, AbortReason, AbortRootContext, AbortStage, ConfigKey,
    ConfigMap, DrainMode, EdgeAction, EdgeContext, EdgeDescriptor, EdgeMetricsSnapshot,
    EdgePolicies, EdgePolicy, EnabledCondition, EnqueueOutcome, GraphDefinition,
    GraphRunnerBuildError, Node, NodeContext, NodeEmission, NodeInstances, NodeKind, PortDirection,
    PortName, QueuePushError, StopToken, TransformPolicy, ValidationDecision,
    ValidationFailureAction, ValidationPolicy,
};

/// Stage 5A scheduler options. Admission is deliberately fixed at one active
/// callback per node; later profiles may lower or raise the declared ceiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeOptions {
    shutdown_mode: DrainMode,
    failure_mode: DrainMode,
    max_in_flight: usize,
}

impl RuntimeOptions {
    /// Creates explicit normal Stop and failure queue-close behavior.
    pub fn new(shutdown_mode: DrainMode, failure_mode: DrainMode) -> Self {
        Self {
            shutdown_mode,
            failure_mode,
            max_in_flight: 1,
        }
    }

    pub const fn shutdown_mode(self) -> DrainMode {
        self.shutdown_mode
    }

    pub const fn failure_mode(self) -> DrainMode {
        self.failure_mode
    }

    pub const fn max_in_flight(self) -> usize {
        self.max_in_flight
    }
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self::new(DrainMode::Discard, DrainMode::Discard)
    }
}

/// Observable concurrent graph lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConcurrentRuntimeState {
    Starting,
    Running,
    Stopping,
    Finishing,
    Aborting,
    Finished,
    Aborted,
}

/// Successful result after all workers and reverse lifecycle hooks complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConcurrentRunSummary {
    worker_total: usize,
}

impl ConcurrentRunSummary {
    pub const fn worker_total(self) -> usize {
        self.worker_total
    }
}

/// Bounded diagnostics returned instead of silently joining forever.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShutdownDiagnostics {
    state: ConcurrentRuntimeState,
    active_nodes: Box<[NodeId]>,
}

impl ShutdownDiagnostics {
    pub const fn state(&self) -> ConcurrentRuntimeState {
        self.state
    }

    pub fn active_nodes(&self) -> &[NodeId] {
        &self.active_nodes
    }
}

/// A bounded wait either completes or reports which workers are still live.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeWaitError {
    Timeout(ShutdownDiagnostics),
    Aborted(AbortReason),
}

impl fmt::Display for RuntimeWaitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout(diagnostics) => write!(
                formatter,
                "timed out waiting for graph shutdown; {} node worker(s) still active",
                diagnostics.active_nodes.len()
            ),
            Self::Aborted(reason) => write!(
                formatter,
                "graph aborted: {}: {}",
                reason.root().code(),
                reason.root().message()
            ),
        }
    }
}

impl std::error::Error for RuntimeWaitError {}

/// Compiled single-use concurrent runtime before worker launch.
pub struct ConcurrentRuntime {
    graph: Arc<GraphDefinition>,
    nodes: NodeInstances,
    policies: EdgePolicies,
    enabled_edges: BTreeSet<EdgeId>,
    options: RuntimeOptions,
}

impl ConcurrentRuntime {
    /// Validates runtime attachments without invoking user code.
    pub fn new(
        graph: GraphDefinition,
        nodes: NodeInstances,
        policies: EdgePolicies,
        options: RuntimeOptions,
    ) -> Result<Self, GraphRunnerBuildError> {
        validate_nodes(&graph, &nodes)?;
        let enabled_edges = enabled_edges(&graph)?;
        let expected = graph
            .edges()
            .iter()
            .filter(|edge| enabled_edges.contains(edge.edge_id()) && uses_named_policy(edge))
            .map(|edge| edge.edge_id().clone())
            .collect::<BTreeSet<_>>();
        validate_policies(&expected, &policies)?;
        Ok(Self {
            graph: Arc::new(graph),
            nodes,
            policies,
            enabled_edges,
            options,
        })
    }

    /// Starts all node execution domains. No Node or EdgePolicy callback runs
    /// on the caller thread after this method is entered.
    pub fn start(self) -> GraphRuntime {
        let stop = StopToken::new();
        let state = Arc::new(RuntimeStatus::new());
        let completion = Arc::new(Completion::default());
        let failure = Arc::new(Mutex::new(None));
        let abort_diagnostics = shared_abort_diagnostics();
        let active = Arc::new(Mutex::new(
            self.graph
                .topological_order()
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
        ));
        let wakes = self
            .graph
            .topological_order()
            .iter()
            .map(|id| (id.clone(), Arc::new(QueueWake::default())))
            .collect::<BTreeMap<_, _>>();
        let mut queues = BTreeMap::new();
        for edge in self.graph.edges() {
            let queue = crate::EdgeQueue::with_target_wake(
                edge.edge_id().clone(),
                edge.queue_policy(),
                wakes.get(edge.to_node_id()).cloned(),
            );
            if !self.enabled_edges.contains(edge.edge_id()) {
                queue.close(DrainMode::Drain);
            }
            queues.insert(edge.edge_id().clone(), queue);
        }
        let all_queues = Arc::new(queues);
        let worker_total = self.graph.nodes().len();
        let gate = Arc::new(PrepareGate::new(worker_total));
        let shared = Arc::new(WorkerShared {
            graph: self.graph.clone(),
            stop: stop.clone(),
            queues: all_queues.clone(),
            failure: failure.clone(),
            state: state.clone(),
            active: active.clone(),
            gate,
            options: self.options,
            abort_diagnostics: abort_diagnostics.clone(),
        });

        let mut incoming = BTreeMap::<NodeId, Vec<InputEdge>>::new();
        let mut outgoing = BTreeMap::<NodeId, Vec<OutputEdge>>::new();
        let mut policies = self.policies;
        for edge in self.graph.edges() {
            if !self.enabled_edges.contains(edge.edge_id()) {
                continue;
            }
            let queue = all_queues
                .get(edge.edge_id())
                .expect("created queue")
                .clone();
            incoming
                .entry(edge.to_node_id().clone())
                .or_default()
                .push(InputEdge {
                    port: edge.to_input_port().clone(),
                    queue: queue.clone(),
                });
            outgoing
                .entry(edge.from_node_id().clone())
                .or_default()
                .push(OutputEdge {
                    descriptor: edge.clone(),
                    queue,
                    policy: policies.remove(edge.edge_id()),
                });
        }

        let (exit_tx, exit_rx) = mpsc::channel();
        let mut worker_handles = Vec::new();
        let mut nodes = self.nodes;
        for node_id in self.graph.topological_order() {
            let worker = NodeWorker {
                node_id: node_id.clone(),
                node: nodes.remove(node_id).expect("validated node"),
                incoming: incoming.remove(node_id).unwrap_or_default(),
                outgoing: outgoing.remove(node_id).unwrap_or_default(),
                wake: wakes.get(node_id).expect("node wake").clone(),
                shared: shared.clone(),
            };
            let tx = exit_tx.clone();
            let name = format!("voxa-node-{}", node_id.as_str());
            worker_handles.push(
                thread::Builder::new()
                    .name(name)
                    .spawn(move || {
                        let exit = worker.run();
                        let _ = tx.send(exit);
                    })
                    .expect("failed to spawn Voxa node worker"),
            );
        }
        drop(exit_tx);

        let coordinator_shared = shared.clone();
        let coordinator_completion = completion.clone();
        thread::Builder::new()
            .name("voxa-runtime-coordinator".to_owned())
            .spawn(move || {
                coordinate(
                    coordinator_shared,
                    exit_rx,
                    worker_handles,
                    coordinator_completion,
                    worker_total,
                );
            })
            .expect("failed to spawn Voxa runtime coordinator");

        GraphRuntime {
            stop,
            queues: all_queues,
            failure,
            state,
            completion,
            active,
            options: self.options,
            abort_diagnostics,
        }
    }
}

/// Thread-safe control and observation handle for a running graph.
#[derive(Clone)]
pub struct GraphRuntime {
    stop: StopToken,
    queues: Arc<BTreeMap<EdgeId, crate::EdgeQueue>>,
    failure: Arc<Mutex<Option<AbortReason>>>,
    state: Arc<RuntimeStatus>,
    completion: Arc<Completion>,
    active: Arc<Mutex<BTreeSet<NodeId>>>,
    options: RuntimeOptions,
    abort_diagnostics: Arc<Mutex<Vec<AbortHookDiagnostic>>>,
}

impl GraphRuntime {
    /// Idempotently stops the graph from any thread and wakes all queue waits.
    /// Returns true only for the call that installed cancellation first.
    pub fn stop(&self) -> bool {
        let reason = cancellation_abort();
        let first = install_failure(&self.failure, reason);
        let cancelled = self.stop.cancel();
        self.state.set(ConcurrentRuntimeState::Stopping);
        close_all(&self.queues, self.options.shutdown_mode);
        first && cancelled
    }

    pub fn state(&self) -> ConcurrentRuntimeState {
        self.state.get()
    }

    pub fn edge_metrics(&self, edge_id: &EdgeId) -> Option<EdgeMetricsSnapshot> {
        self.queues.get(edge_id).map(crate::EdgeQueue::snapshot)
    }

    pub fn abort_diagnostics(&self) -> Vec<AbortHookDiagnostic> {
        self.abort_diagnostics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Waits with an explicit deadline. A timeout never hides live workers.
    pub fn wait(&self, timeout: Duration) -> Result<ConcurrentRunSummary, RuntimeWaitError> {
        let mut result = self
            .completion
            .result
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if result.is_none() {
            let waited = self
                .completion
                .changed
                .wait_timeout_while(result, timeout, |value| value.is_none())
                .unwrap_or_else(|e| e.into_inner());
            result = waited.0;
        }
        match result.as_ref() {
            Some(Ok(summary)) => Ok(*summary),
            Some(Err(reason)) => Err(RuntimeWaitError::Aborted(reason.clone())),
            None => Err(RuntimeWaitError::Timeout(ShutdownDiagnostics {
                state: self.state(),
                active_nodes: self
                    .active
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .cloned()
                    .collect(),
            })),
        }
    }
}

struct RuntimeStatus(Mutex<ConcurrentRuntimeState>);

impl RuntimeStatus {
    fn new() -> Self {
        Self(Mutex::new(ConcurrentRuntimeState::Starting))
    }

    fn get(&self) -> ConcurrentRuntimeState {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set(&self, next: ConcurrentRuntimeState) {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(
            *state,
            ConcurrentRuntimeState::Finished | ConcurrentRuntimeState::Aborted
        ) {
            return;
        }
        *state = next;
    }
}

#[derive(Default)]
struct Completion {
    result: Mutex<Option<Result<ConcurrentRunSummary, AbortReason>>>,
    changed: Condvar,
}

struct PrepareGate {
    state: Mutex<PrepareState>,
    changed: Condvar,
}

struct PrepareState {
    remaining: usize,
    released: bool,
}

impl PrepareGate {
    fn new(workers: usize) -> Self {
        Self {
            state: Mutex::new(PrepareState {
                remaining: workers,
                released: workers == 0,
            }),
            changed: Condvar::new(),
        }
    }

    fn arrive_and_wait(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.remaining = state.remaining.saturating_sub(1);
        if state.remaining == 0 {
            state.released = true;
            self.changed.notify_all();
        }
        while !state.released {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

struct WorkerShared {
    graph: Arc<GraphDefinition>,
    stop: StopToken,
    queues: Arc<BTreeMap<EdgeId, crate::EdgeQueue>>,
    failure: Arc<Mutex<Option<AbortReason>>>,
    state: Arc<RuntimeStatus>,
    active: Arc<Mutex<BTreeSet<NodeId>>>,
    gate: Arc<PrepareGate>,
    options: RuntimeOptions,
    abort_diagnostics: Arc<Mutex<Vec<AbortHookDiagnostic>>>,
}

struct InputEdge {
    port: PortName,
    queue: crate::EdgeQueue,
}

struct OutputEdge {
    descriptor: EdgeDescriptor,
    queue: crate::EdgeQueue,
    policy: Option<Box<dyn EdgePolicy>>,
}

struct NodeWorker {
    node_id: NodeId,
    node: Box<dyn Node>,
    incoming: Vec<InputEdge>,
    outgoing: Vec<OutputEdge>,
    wake: Arc<QueueWake>,
    shared: Arc<WorkerShared>,
}

struct WorkerExit {
    node_id: NodeId,
    node: Box<dyn Node>,
    outgoing: Vec<OutputEdge>,
    prepared: bool,
}

impl NodeWorker {
    fn run(mut self) -> WorkerExit {
        let prepared = match call_prepare(&mut *self.node, &self.node_id, &self.shared.graph) {
            Ok(()) => true,
            Err(reason) => {
                fail_graph(&self.shared, reason);
                false
            }
        };
        self.shared.gate.arrive_and_wait();
        self.shared.state.set(ConcurrentRuntimeState::Running);

        let is_source = self
            .shared
            .graph
            .node(&self.node_id)
            .expect("validated node")
            .descriptor()
            .kind()
            == NodeKind::Source;
        if prepared && (!is_source || !self.shared.stop.is_cancelled()) {
            let result = if is_source {
                self.run_source()
            } else {
                self.run_consumer()
            };
            if let Err(reason) = result {
                fail_graph(&self.shared, reason);
            }
        }

        for output in &self.outgoing {
            output.queue.close(DrainMode::Drain);
        }
        self.shared
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.node_id);
        WorkerExit {
            node_id: self.node_id,
            node: self.node,
            outgoing: self.outgoing,
            prepared,
        }
    }

    fn run_source(&mut self) -> Result<(), AbortReason> {
        let emissions = call_process(
            &mut *self.node,
            &self.node_id,
            None,
            None,
            &self.shared.graph,
        )?;
        self.route(emissions)
    }

    fn run_consumer(&mut self) -> Result<(), AbortReason> {
        let mut cursor = 0usize;
        loop {
            let observed = self.wake.generation();
            let mut received = None;
            for offset in 0..self.incoming.len() {
                let index = (cursor + offset) % self.incoming.len();
                if let Ok(Some(frame)) = self.incoming[index].queue.try_pop() {
                    received = Some((index, frame));
                    cursor = (index + 1) % self.incoming.len();
                    break;
                }
            }
            if let Some((index, frame)) = received {
                let input_port = self.incoming[index].port.clone();
                let emissions = call_process(
                    &mut *self.node,
                    &self.node_id,
                    Some(frame),
                    Some(input_port),
                    &self.shared.graph,
                )?;
                self.route(emissions)?;
                continue;
            }
            if self
                .incoming
                .iter()
                .all(|edge| edge.queue.is_closed_and_empty())
            {
                return Ok(());
            }
            self.wake.wait_for_change(observed);
        }
    }

    fn route(&mut self, emissions: Vec<NodeEmission>) -> Result<(), AbortReason> {
        let descriptor = self
            .shared
            .graph
            .node(&self.node_id)
            .expect("validated node")
            .descriptor();
        for emission in emissions {
            let (output_port, frame) = emission.into_parts();
            let Some(port) = descriptor.ports().iter().find(|candidate| {
                candidate.name() == &output_port && candidate.direction() == PortDirection::Output
            }) else {
                return Err(runtime_abort_details(
                    "VOXA-CONCURRENT-OUTPUT-PORT",
                    "node emitted through an undeclared output port",
                    Some(self.node_id.clone()),
                    [("output_port", output_port.as_str())],
                ));
            };
            if frame.frame_type() != port.frame_type() {
                return Err(runtime_abort(
                    "VOXA-CONCURRENT-OUTPUT-TYPE",
                    "node emitted a frame whose type does not match its output port",
                    Some(self.node_id.clone()),
                    AbortCategory::NodeError,
                    AbortStage::Process,
                ));
            }
            for output in self
                .outgoing
                .iter_mut()
                .filter(|edge| edge.descriptor.from_output_port() == &output_port)
            {
                apply_edge(&self.shared.graph, output, frame.clone())?;
            }
        }
        Ok(())
    }
}

fn coordinate(
    shared: Arc<WorkerShared>,
    exits: mpsc::Receiver<WorkerExit>,
    handles: Vec<thread::JoinHandle<()>>,
    completion: Arc<Completion>,
    worker_total: usize,
) {
    let mut nodes = BTreeMap::new();
    let mut resources = Vec::new();
    let mut prepared = BTreeSet::new();
    for exit in exits {
        if exit.prepared {
            prepared.insert(exit.node_id.clone());
        }
        resources.push(exit.outgoing);
        nodes.insert(exit.node_id, exit.node);
    }
    for handle in handles {
        let _ = handle.join();
    }

    let mut reason = shared
        .failure
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if reason.is_none() {
        shared.state.set(ConcurrentRuntimeState::Finishing);
        for node_id in shared.graph.topological_order().iter().rev() {
            if let Some(node) = nodes.get_mut(node_id) {
                if let Err(error) = call_finish(&mut **node, node_id, &shared.graph) {
                    reason = Some(error.clone());
                    install_failure(&shared.failure, error);
                    shared.stop.cancel();
                    close_all(&shared.queues, shared.options.failure_mode);
                    break;
                }
            }
        }
        if reason.is_none() {
            reason = shared
                .failure
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
        }
    }

    let result = if let Some(reason) = reason {
        shared.state.set(ConcurrentRuntimeState::Aborting);
        let diagnostics = shared.abort_diagnostics.clone();
        for node_id in shared.graph.topological_order().iter().rev() {
            if !prepared.contains(node_id) {
                continue;
            }
            let config = shared.graph.node(node_id).expect("node").config().clone();
            let mut context = NodeContext::new(node_id.clone(), config, None);
            if let Some(node) = nodes.get_mut(node_id) {
                if let Err(payload) =
                    catch_unwind(AssertUnwindSafe(|| node.on_abort(&reason, &mut context)))
                {
                    diagnostics.lock().unwrap_or_else(|e| e.into_inner()).push(
                        AbortHookDiagnostic::new(node_id.clone(), panic_message(payload.as_ref())),
                    );
                }
            }
        }
        shared.state.set(ConcurrentRuntimeState::Aborted);
        Err(reason)
    } else {
        shared.state.set(ConcurrentRuntimeState::Finished);
        Ok(ConcurrentRunSummary { worker_total })
    };

    drop(nodes);
    drop(resources);
    let mut slot = completion.result.lock().unwrap_or_else(|e| e.into_inner());
    *slot = Some(result);
    completion.changed.notify_all();
}

fn call_prepare(
    node: &mut dyn Node,
    node_id: &NodeId,
    graph: &GraphDefinition,
) -> Result<(), AbortReason> {
    let config = graph.node(node_id).expect("node").config().clone();
    let mut context = NodeContext::new(node_id.clone(), config, None);
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
    node: &mut dyn Node,
    node_id: &NodeId,
    input: Option<Frame>,
    input_port: Option<PortName>,
    graph: &GraphDefinition,
) -> Result<Vec<NodeEmission>, AbortReason> {
    let config = graph.node(node_id).expect("node").config().clone();
    let mut context = NodeContext::new(node_id.clone(), config, input_port);
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

fn call_finish(
    node: &mut dyn Node,
    node_id: &NodeId,
    graph: &GraphDefinition,
) -> Result<(), AbortReason> {
    let config = graph.node(node_id).expect("node").config().clone();
    let mut context = NodeContext::new(node_id.clone(), config, None);
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

fn apply_edge(
    graph: &GraphDefinition,
    output: &mut OutputEdge,
    original: Frame,
) -> Result<(), AbortReason> {
    let edge = &output.descriptor;
    if original.frame_type() != edge.frame_type() {
        output
            .queue
            .record_error("frame failed the mandatory exact Edge type gate");
        return Err(edge_abort(
            edge,
            "VOXA-CONCURRENT-EDGE-TYPE",
            "frame failed the mandatory exact Edge type gate",
            "type_gate",
        ));
    }

    let validation = match edge.validation_policy() {
        ValidationPolicy::TypeGateOnly => ValidationDecision::Accept,
        ValidationPolicy::Named { .. } => {
            let snapshot = output.queue.snapshot();
            let context = EdgeContext::new(graph, edge, &snapshot);
            let policy = output.policy.as_mut().expect("validated policy");
            match catch_unwind(AssertUnwindSafe(|| policy.validate(&original, &context))) {
                Ok(Ok(decision)) => decision,
                Ok(Err(error)) => return Err(policy_error(edge, &output.queue, error, "validate")),
                Err(payload) => return Err(policy_panic(edge, &output.queue, payload, "validate")),
            }
        }
    };
    if let ValidationDecision::Reject(reason) = validation {
        return match edge.validation_policy() {
            ValidationPolicy::Named {
                on_failure: ValidationFailureAction::Abort,
                ..
            } => {
                output.queue.record_error(&reason);
                Err(edge_abort(
                    edge,
                    "VOXA-CONCURRENT-VALIDATION",
                    &reason,
                    "validate",
                ))
            }
            _ => {
                record_policy_drop(graph, output, &reason)?;
                Ok(())
            }
        };
    }

    let action = match edge.transform_policy() {
        TransformPolicy::Identity => EdgeAction::Forward(original.clone()),
        TransformPolicy::Named(_) => {
            let snapshot = output.queue.snapshot();
            let context = EdgeContext::new(graph, edge, &snapshot);
            let policy = output.policy.as_mut().expect("validated policy");
            match catch_unwind(AssertUnwindSafe(|| policy.transform(&original, &context))) {
                Ok(Ok(action)) => action,
                Ok(Err(error)) => {
                    return Err(policy_error(edge, &output.queue, error, "transform"))
                }
                Err(payload) => {
                    return Err(policy_panic(edge, &output.queue, payload, "transform"))
                }
            }
        }
    };

    let frame = match action {
        EdgeAction::Forward(frame) => {
            if frame != original || frame.header().frame_id() != original.header().frame_id() {
                return Err(edge_abort(
                    edge,
                    "VOXA-CONCURRENT-FORWARD-MUTATION",
                    "Forward must return the unchanged frame",
                    "transform",
                ));
            }
            frame
        }
        EdgeAction::Replace(replacement) => {
            if replacement.header().frame_id() == original.header().frame_id()
                || replacement.frame_type() != edge.frame_type()
            {
                return Err(edge_abort(
                    edge,
                    "VOXA-CONCURRENT-REPLACE",
                    "Replace requires a fresh ID and the exact Edge type",
                    "transform",
                ));
            }
            let origin = TransformOrigin::new(None, Some(edge.edge_id().clone()))
                .map_err(|error| node_error(error, edge.from_node_id(), AbortStage::Runtime))?;
            original
                .attach_replacement_lineage(replacement, origin, "edge policy replacement")
                .map_err(|error| node_error(error, edge.from_node_id(), AbortStage::Runtime))?
        }
        EdgeAction::Drop(reason) => {
            record_policy_drop(graph, output, &reason)?;
            return Ok(());
        }
        EdgeAction::Abort(reason) => {
            output.queue.record_error(&reason);
            return Err(edge_abort(
                edge,
                "VOXA-CONCURRENT-POLICY-ABORT",
                &reason,
                "transform",
            ));
        }
        EdgeAction::EmitSignal(frame) => {
            if frame.frame_type() != voxa_types::FrameType::Signal {
                return Err(edge_abort(
                    edge,
                    "VOXA-CONCURRENT-SIGNAL-TYPE",
                    "EmitSignal requires a Signal frame",
                    "transform",
                ));
            }
            output.queue.record_signal();
            let snapshot = output.queue.snapshot();
            let context = EdgeContext::new(graph, edge, &snapshot);
            let signal = frame.as_signal().expect("type checked");
            let policy = output.policy.as_mut().expect("named policy");
            match catch_unwind(AssertUnwindSafe(|| policy.on_signal(signal, &context))) {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(error)) => {
                    return Err(policy_error(edge, &output.queue, error, "on_signal"))
                }
                Err(payload) => {
                    return Err(policy_panic(edge, &output.queue, payload, "on_signal"))
                }
            }
        }
    };

    if frame.frame_type() != edge.frame_type() {
        return Err(edge_abort(
            edge,
            "VOXA-CONCURRENT-QUEUE-TYPE",
            "policy output type does not match queue type",
            "queue",
        ));
    }
    match output.queue.push(frame) {
        Ok(
            EnqueueOutcome::Enqueued
            | EnqueueOutcome::EnqueuedAfterDroppingOldest
            | EnqueueOutcome::Dropped(_),
        ) => Ok(()),
        Err(QueuePushError::OverflowAbort) => Err(edge_abort(
            edge,
            "VOXA-CONCURRENT-QUEUE-ABORT",
            "queue overflow policy aborted",
            "queue",
        )),
        Err(QueuePushError::Closed) => Err(edge_abort(
            edge,
            "VOXA-CONCURRENT-QUEUE-CLOSED",
            "queue closed while routing a frame",
            "queue",
        )),
    }
}

fn record_policy_drop(
    graph: &GraphDefinition,
    output: &mut OutputEdge,
    reason: &str,
) -> Result<(), AbortReason> {
    output.queue.record_drop(reason);
    let snapshot = output.queue.snapshot();
    let context = EdgeContext::new(graph, &output.descriptor, &snapshot);
    let policy = output
        .policy
        .as_mut()
        .expect("policy drop requires named policy");
    match catch_unwind(AssertUnwindSafe(|| policy.on_drop(reason, &context))) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(policy_error(
            &output.descriptor,
            &output.queue,
            error,
            "on_drop",
        )),
        Err(payload) => Err(policy_panic(
            &output.descriptor,
            &output.queue,
            payload,
            "on_drop",
        )),
    }
}

fn fail_graph(shared: &WorkerShared, reason: AbortReason) {
    if install_failure(&shared.failure, reason) {
        shared.state.set(ConcurrentRuntimeState::Stopping);
        shared.stop.cancel();
        close_all(&shared.queues, shared.options.failure_mode);
    }
}

fn install_failure(slot: &Mutex<Option<AbortReason>>, reason: AbortReason) -> bool {
    let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        false
    } else {
        *slot = Some(reason);
        true
    }
}

fn close_all(queues: &BTreeMap<EdgeId, crate::EdgeQueue>, mode: DrainMode) {
    for queue in queues.values() {
        queue.close(mode);
    }
}

fn validate_nodes(
    graph: &GraphDefinition,
    nodes: &NodeInstances,
) -> Result<(), GraphRunnerBuildError> {
    let expected = graph
        .nodes()
        .iter()
        .map(|node| node.descriptor().node_id().clone())
        .collect::<BTreeSet<_>>();
    let actual = nodes.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(id) = expected.difference(&actual).next() {
        return Err(GraphRunnerBuildError::MissingNodeInstance(id.clone()));
    }
    if let Some(id) = actual.difference(&expected).next() {
        return Err(GraphRunnerBuildError::UnknownNodeInstance(id.clone()));
    }
    Ok(())
}

fn validate_policies(
    expected: &BTreeSet<EdgeId>,
    policies: &EdgePolicies,
) -> Result<(), GraphRunnerBuildError> {
    let actual = policies.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(id) = expected.difference(&actual).next() {
        return Err(GraphRunnerBuildError::MissingEdgePolicy(id.clone()));
    }
    if let Some(id) = actual.difference(expected).next() {
        return Err(GraphRunnerBuildError::UnknownEdgePolicy(id.clone()));
    }
    Ok(())
}

fn enabled_edges(graph: &GraphDefinition) -> Result<BTreeSet<EdgeId>, GraphRunnerBuildError> {
    let mut enabled = BTreeSet::new();
    for edge in graph.edges() {
        let active = match edge.enabled() {
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
        if active {
            enabled.insert(edge.edge_id().clone());
        }
    }
    Ok(enabled)
}

fn uses_named_policy(edge: &EdgeDescriptor) -> bool {
    matches!(edge.validation_policy(), ValidationPolicy::Named { .. })
        || matches!(edge.transform_policy(), TransformPolicy::Named(_))
}

fn node_error(error: VoxaError, node_id: &NodeId, stage: AbortStage) -> AbortReason {
    let category = match error.category() {
        ErrorCategory::Cancelled => AbortCategory::Cancelled,
        ErrorCategory::External => AbortCategory::ExternalSdkError,
        _ => AbortCategory::NodeError,
    };
    runtime_abort(
        error.code(),
        error.message(),
        Some(node_id.clone()),
        category,
        stage,
    )
}

fn cancellation_abort() -> AbortReason {
    runtime_abort(
        "VOXA-CONCURRENT-CANCELLED",
        "graph Stop was requested",
        None,
        AbortCategory::Cancelled,
        AbortStage::Runtime,
    )
}

fn edge_abort(edge: &EdgeDescriptor, code: &str, message: &str, phase: &str) -> AbortReason {
    AbortReason::new(
        AbortCategory::NodeError,
        Some(edge.from_node_id().clone()),
        AbortStage::Runtime,
        AbortRootContext::new(
            code,
            bounded(message),
            details([
                ("edge_id", edge.edge_id().as_str()),
                ("policy_phase", phase),
            ]),
        ),
    )
}

fn policy_error(
    edge: &EdgeDescriptor,
    queue: &crate::EdgeQueue,
    error: VoxaError,
    phase: &str,
) -> AbortReason {
    queue.record_error(error.message());
    let category = match error.category() {
        ErrorCategory::Cancelled => AbortCategory::Cancelled,
        ErrorCategory::External => AbortCategory::ExternalSdkError,
        _ => AbortCategory::NodeError,
    };
    AbortReason::new(
        category,
        Some(edge.from_node_id().clone()),
        AbortStage::Runtime,
        AbortRootContext::new(
            error.code(),
            bounded(error.message()),
            details([
                ("edge_id", edge.edge_id().as_str()),
                ("policy_phase", phase),
            ]),
        ),
    )
}

fn policy_panic(
    edge: &EdgeDescriptor,
    queue: &crate::EdgeQueue,
    payload: Box<dyn Any + Send>,
    phase: &str,
) -> AbortReason {
    let message = panic_message(payload.as_ref());
    queue.record_error(&message);
    panic_abort(
        payload,
        Some(edge.from_node_id()),
        AbortStage::Runtime,
        Some((edge.edge_id(), phase)),
    )
}

fn panic_abort(
    payload: Box<dyn Any + Send>,
    node_id: Option<&NodeId>,
    stage: AbortStage,
    edge: Option<(&EdgeId, &str)>,
) -> AbortReason {
    let data = edge.map_or_else(ConfigMap::empty, |(id, phase)| {
        details([("edge_id", id.as_str()), ("policy_phase", phase)])
    });
    AbortReason::new(
        AbortCategory::RustPanic,
        node_id.cloned(),
        stage,
        AbortRootContext::new(
            "VOXA-CONCURRENT-PANIC",
            panic_message(payload.as_ref()),
            data,
        ),
    )
}

fn runtime_abort(
    code: &str,
    message: &str,
    node_id: Option<NodeId>,
    category: AbortCategory,
    stage: AbortStage,
) -> AbortReason {
    AbortReason::new(
        category,
        node_id,
        stage,
        AbortRootContext::new(code, bounded(message), ConfigMap::empty()),
    )
}

fn runtime_abort_details<const N: usize>(
    code: &str,
    message: &str,
    node_id: Option<NodeId>,
    values: [(&str, &str); N],
) -> AbortReason {
    AbortReason::new(
        AbortCategory::NodeError,
        node_id,
        AbortStage::Process,
        AbortRootContext::new(code, bounded(message), details(values)),
    )
}

fn panic_message(payload: &(dyn Any + Send)) -> Box<str> {
    bounded(if let Some(value) = payload.downcast_ref::<&str>() {
        value
    } else if let Some(value) = payload.downcast_ref::<String>() {
        value
    } else {
        "Rust task panicked with a non-string payload"
    })
}

fn bounded(message: &str) -> Box<str> {
    const MAX: usize = 256;
    let mut value = String::new();
    for character in message.chars() {
        let character = if character.is_ascii_control() {
            ' '
        } else {
            character
        };
        if value.len() + character.len_utf8() > MAX {
            break;
        }
        value.push(character);
    }
    value.into_boxed_str()
}

fn details<const N: usize>(values: [(&str, &str); N]) -> ConfigMap {
    ConfigMap::try_from_iter(values.map(|(key, value)| {
        (
            ConfigKey::new(key).expect("valid detail key"),
            Value::String(Box::from(value)),
        )
    }))
    .expect("unique detail keys")
}

fn shared_abort_diagnostics() -> Arc<Mutex<Vec<AbortHookDiagnostic>>> {
    Arc::new(Mutex::new(Vec::new()))
}
