use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fmt, io,
    num::NonZeroUsize,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{mpsc, Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

use voxa_types::{
    EdgeId, ErrorCategory, Frame, NodeId, SignalFrame, TransformOrigin, TurnId, Value, VoxaError,
};

use crate::queue::QueueWake;
use crate::{
    AbortCategory, AbortHookDiagnostic, AbortReason, AbortRootContext, AbortStage, ConfigKey,
    ConfigMap, DrainMode, EdgeAction, EdgeContext, EdgeDescriptor, EdgeMetricsSnapshot,
    EdgePolicies, EdgePolicy, EnabledCondition, EnqueueOutcome, GraphDefinition,
    GraphRunnerBuildError, Node, NodeContext, NodeEmission, NodeInstances, NodeKind, PortDirection,
    PortName, QueuePushError, ResourceStore, SignalQueuePushError, StopToken, TransformPolicy,
    TransportControl, ValidationDecision, ValidationFailureAction, ValidationPolicy,
};
use crate::{EventBus, SignalQueueSnapshot};

/// Stage 5A scheduler options. Admission is deliberately fixed at one active
/// callback per node; later profiles may lower or raise the declared ceiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeOptions {
    shutdown_mode: DrainMode,
    failure_mode: DrainMode,
    max_in_flight: usize,
    emission_budget: NonZeroUsize,
    signal_queue_capacity: NonZeroUsize,
}

impl RuntimeOptions {
    /// Creates explicit normal Stop and failure queue-close behavior.
    pub fn new(shutdown_mode: DrainMode, failure_mode: DrainMode) -> Self {
        Self {
            shutdown_mode,
            failure_mode,
            max_in_flight: 1,
            emission_budget: NonZeroUsize::new(16_384).expect("non-zero constant"),
            signal_queue_capacity: NonZeroUsize::new(64).expect("non-zero constant"),
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

    /// Sets the maximum emissions retained from one Node lifecycle call.
    pub const fn with_emission_budget(mut self, emission_budget: NonZeroUsize) -> Self {
        self.emission_budget = emission_budget;
        self
    }

    pub const fn emission_budget(self) -> usize {
        self.emission_budget.get()
    }

    pub const fn with_signal_queue_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.signal_queue_capacity = capacity;
        self
    }

    pub const fn signal_queue_capacity(self) -> usize {
        self.signal_queue_capacity.get()
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

/// Execution-domain kind whose creation prevented a runtime from starting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeThreadRole {
    EdgeDispatcher,
    NodeWorker,
    Coordinator,
}

/// Structured, recoverable failure to create all runtime execution domains.
#[derive(Debug)]
pub struct RuntimeStartError {
    role: RuntimeThreadRole,
    thread_name: Box<str>,
    source: io::Error,
}

impl RuntimeStartError {
    pub const fn role(&self) -> RuntimeThreadRole {
        self.role
    }

    pub fn thread_name(&self) -> &str {
        &self.thread_name
    }
}

impl fmt::Display for RuntimeStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "failed to spawn {:?} thread `{}`: {}",
            self.role, self.thread_name, self.source
        )
    }
}

impl std::error::Error for RuntimeStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Compiled single-use concurrent runtime before worker launch.
pub struct ConcurrentRuntime {
    graph: Arc<GraphDefinition>,
    nodes: NodeInstances,
    policies: EdgePolicies,
    enabled_edges: BTreeSet<EdgeId>,
    options: RuntimeOptions,
    event_bus: EventBus,
    resources: ResourceStore,
    transport: TransportControl,
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
            event_bus: EventBus::default(),
            resources: ResourceStore::new(),
            transport: TransportControl::new(
                TurnId::new("turn.initial").expect("valid static turn ID"),
            ),
        })
    }

    pub fn with_event_bus(mut self, event_bus: EventBus) -> Self {
        self.event_bus = event_bus;
        self
    }

    pub fn with_resources(mut self, resources: ResourceStore) -> Self {
        self.resources = resources;
        self
    }

    pub fn with_transport_control(mut self, transport: TransportControl) -> Self {
        self.transport = transport;
        self
    }

    /// Starts all node execution domains. No Node or EdgePolicy callback runs
    /// on the caller thread after this method is entered.
    pub fn start(self) -> Result<GraphRuntime, RuntimeStartError> {
        self.start_with_spawner(&SystemThreadSpawner)
    }

    fn start_with_spawner(
        self,
        spawner: &dyn ThreadSpawner,
    ) -> Result<GraphRuntime, RuntimeStartError> {
        let stop = StopToken::new();
        let control = Arc::new(RuntimeControl::new(
            self.graph.topological_order().iter().cloned(),
        ));
        let abort_diagnostics = shared_abort_diagnostics();
        let launch = Arc::new(LaunchGate::default());
        let wakes = self
            .graph
            .topological_order()
            .iter()
            .map(|id| (id.clone(), Arc::new(QueueWake::default())))
            .collect::<BTreeMap<_, _>>();
        let mut queues = BTreeMap::new();
        let mut signal_queues = BTreeMap::new();
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
            let signal_queue = crate::signal::SignalQueue::new(
                self.options.signal_queue_capacity(),
                wakes.get(edge.to_node_id()).expect("target wake").clone(),
            );
            if !self.enabled_edges.contains(edge.edge_id()) {
                signal_queue.close(DrainMode::Drain);
            }
            signal_queues.insert(edge.edge_id().clone(), signal_queue);
        }
        let all_queues = Arc::new(queues);
        let all_signal_queues = Arc::new(signal_queues);
        let worker_total = self.graph.nodes().len();
        let gate = Arc::new(PrepareGate::new(worker_total));
        let shared = Arc::new(WorkerShared {
            graph: self.graph.clone(),
            stop: stop.clone(),
            queues: all_queues.clone(),
            signal_queues: all_signal_queues.clone(),
            control: control.clone(),
            gate,
            launch: launch.clone(),
            options: self.options,
            abort_diagnostics: abort_diagnostics.clone(),
            event_bus: self.event_bus.clone(),
            resources: self.resources.clone(),
            transport: self.transport.clone(),
        });

        let mut incoming = BTreeMap::<NodeId, Vec<InputEdge>>::new();
        let mut outgoing = BTreeMap::<NodeId, Vec<OutputDispatch>>::new();
        let mut dispatchers = Vec::new();
        let mut policies = self.policies;
        for edge in self.graph.edges() {
            if !self.enabled_edges.contains(edge.edge_id()) {
                continue;
            }
            let queue = all_queues
                .get(edge.edge_id())
                .expect("created queue")
                .clone();
            let signal_queue = all_signal_queues
                .get(edge.edge_id())
                .expect("created signal queue")
                .clone();
            incoming
                .entry(edge.to_node_id().clone())
                .or_default()
                .push(InputEdge {
                    port: edge.to_input_port().clone(),
                    queue: queue.clone(),
                    signal_queue: signal_queue.clone(),
                });
            let (sender, receiver) = mpsc::sync_channel(1);
            outgoing
                .entry(edge.from_node_id().clone())
                .or_default()
                .push(OutputDispatch {
                    descriptor: edge.clone(),
                    sender,
                });
            dispatchers.push((
                OutputEdge {
                    descriptor: edge.clone(),
                    queue,
                    signal_queue,
                    policy: policies.remove(edge.edge_id()),
                },
                receiver,
            ));
        }

        let (exit_tx, exit_rx) = mpsc::channel();
        let mut handles = Vec::new();
        while let Some((output, receiver)) = dispatchers.pop() {
            let name = format!("voxa-edge-{}", output.descriptor.edge_id().as_str());
            let worker_shared = shared.clone();
            let tx = exit_tx.clone();
            let task = Box::new(move || {
                if !worker_shared.launch.wait() {
                    let _ = tx.send(RuntimeExit::Edge(Box::new(output)));
                    return;
                }
                let output = run_dispatcher(output, receiver, &worker_shared);
                let _ = tx.send(RuntimeExit::Edge(Box::new(output)));
            });
            match spawner.spawn(name.clone(), task) {
                Ok(handle) => handles.push(handle),
                Err(source) => {
                    drop(dispatchers);
                    drop(outgoing);
                    drop(incoming);
                    cleanup_failed_start(&shared, handles);
                    return Err(RuntimeStartError {
                        role: RuntimeThreadRole::EdgeDispatcher,
                        thread_name: name.into(),
                        source,
                    });
                }
            }
        }

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
            let worker_shared = shared.clone();
            let task = Box::new(move || {
                if !worker_shared.launch.wait() {
                    let _ = tx.send(RuntimeExit::Node(worker.cancelled_exit()));
                    return;
                }
                let _ = tx.send(RuntimeExit::Node(worker.run()));
            });
            match spawner.spawn(name.clone(), task) {
                Ok(handle) => handles.push(handle),
                Err(source) => {
                    drop(nodes);
                    drop(outgoing);
                    drop(incoming);
                    cleanup_failed_start(&shared, handles);
                    return Err(RuntimeStartError {
                        role: RuntimeThreadRole::NodeWorker,
                        thread_name: name.into(),
                        source,
                    });
                }
            }
        }
        drop(exit_tx);

        let handle_registry = Arc::new(Mutex::new(Some(handles)));
        let coordinator_shared = shared.clone();
        let coordinator_handles = handle_registry.clone();
        let coordinator_name = "voxa-runtime-coordinator".to_owned();
        let task = Box::new(move || {
            let handles = coordinator_handles
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .unwrap_or_default();
            coordinate(coordinator_shared, exit_rx, handles, worker_total);
        });
        if let Err(source) = spawner.spawn(coordinator_name.clone(), task) {
            drop(outgoing);
            drop(incoming);
            let handles = handle_registry
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .unwrap_or_default();
            cleanup_failed_start(&shared, handles);
            return Err(RuntimeStartError {
                role: RuntimeThreadRole::Coordinator,
                thread_name: coordinator_name.into(),
                source,
            });
        }

        launch.release();
        Ok(GraphRuntime {
            stop,
            queues: all_queues,
            signal_queues: all_signal_queues,
            control,
            options: self.options,
            abort_diagnostics,
            event_bus: self.event_bus,
            resources: self.resources,
            transport: self.transport,
        })
    }
}

/// Thread-safe control and observation handle for a running graph.
#[derive(Clone)]
pub struct GraphRuntime {
    stop: StopToken,
    queues: Arc<BTreeMap<EdgeId, crate::EdgeQueue>>,
    signal_queues: Arc<BTreeMap<EdgeId, crate::signal::SignalQueue>>,
    control: Arc<RuntimeControl>,
    options: RuntimeOptions,
    abort_diagnostics: Arc<Mutex<Vec<AbortHookDiagnostic>>>,
    event_bus: EventBus,
    resources: ResourceStore,
    transport: TransportControl,
}

impl GraphRuntime {
    /// Idempotently stops the graph from any thread and wakes all queue waits.
    /// Returns true only for the call that installed cancellation first.
    pub fn stop(&self) -> bool {
        let reason = cancellation_abort();
        let first = self.control.request_abort(reason);
        if first {
            self.stop.cancel();
            self.event_bus.request_stop();
            self.resources.seal();
            close_all(&self.queues, self.options.shutdown_mode);
            close_all_signals(&self.signal_queues, self.options.shutdown_mode);
        }
        first
    }

    pub fn state(&self) -> ConcurrentRuntimeState {
        self.control.state()
    }

    pub fn edge_metrics(&self, edge_id: &EdgeId) -> Option<EdgeMetricsSnapshot> {
        self.queues.get(edge_id).map(crate::EdgeQueue::snapshot)
    }

    pub fn signal_metrics(&self, edge_id: &EdgeId) -> Option<SignalQueueSnapshot> {
        self.signal_queues
            .get(edge_id)
            .map(crate::signal::SignalQueue::snapshot)
    }

    pub const fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    pub const fn resources(&self) -> &ResourceStore {
        &self.resources
    }

    pub const fn transport_control(&self) -> &TransportControl {
        &self.transport
    }

    pub fn abort_diagnostics(&self) -> Vec<AbortHookDiagnostic> {
        self.abort_diagnostics
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Waits with an explicit deadline. A timeout never hides live workers.
    pub fn wait(&self, timeout: Duration) -> Result<ConcurrentRunSummary, RuntimeWaitError> {
        let mut inner = self.control.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.result.is_none() {
            inner = self
                .control
                .changed
                .wait_timeout_while(inner, timeout, |value| value.result.is_none())
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        match inner.result.as_ref() {
            Some(Ok(summary)) => Ok(*summary),
            Some(Err(reason)) => Err(RuntimeWaitError::Aborted(reason.clone())),
            None => Err(RuntimeWaitError::Timeout(ShutdownDiagnostics {
                state: inner.state,
                active_nodes: inner.active_nodes.iter().cloned().collect(),
            })),
        }
    }
}

trait ThreadSpawner {
    fn spawn(
        &self,
        name: String,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>>;
}

struct SystemThreadSpawner;

impl ThreadSpawner for SystemThreadSpawner {
    fn spawn(
        &self,
        name: String,
        task: Box<dyn FnOnce() + Send + 'static>,
    ) -> io::Result<thread::JoinHandle<()>> {
        thread::Builder::new().name(name).spawn(task)
    }
}

struct RuntimeControl {
    inner: Mutex<ControlState>,
    changed: Condvar,
}

struct ControlState {
    state: ConcurrentRuntimeState,
    reason: Option<AbortReason>,
    success_sealed: bool,
    result: Option<Result<ConcurrentRunSummary, AbortReason>>,
    active_nodes: BTreeSet<NodeId>,
}

impl RuntimeControl {
    fn new(active_nodes: impl IntoIterator<Item = NodeId>) -> Self {
        Self {
            inner: Mutex::new(ControlState {
                state: ConcurrentRuntimeState::Starting,
                reason: None,
                success_sealed: false,
                result: None,
                active_nodes: active_nodes.into_iter().collect(),
            }),
            changed: Condvar::new(),
        }
    }

    fn state(&self) -> ConcurrentRuntimeState {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).state
    }

    fn mark_running(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.state == ConcurrentRuntimeState::Starting && inner.reason.is_none() {
            inner.state = ConcurrentRuntimeState::Running;
        }
    }

    fn request_abort(&self, reason: AbortReason) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.reason.is_some() || inner.success_sealed || inner.result.is_some() {
            return false;
        }
        inner.reason = Some(reason);
        inner.state = if inner.state == ConcurrentRuntimeState::Finishing {
            ConcurrentRuntimeState::Aborting
        } else {
            ConcurrentRuntimeState::Stopping
        };
        true
    }

    fn reason(&self) -> Option<AbortReason> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .reason
            .clone()
    }

    fn begin_finishing(&self) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.reason.is_some() || inner.result.is_some() {
            return false;
        }
        if matches!(
            inner.state,
            ConcurrentRuntimeState::Starting | ConcurrentRuntimeState::Running
        ) {
            inner.state = ConcurrentRuntimeState::Finishing;
        }
        inner.state == ConcurrentRuntimeState::Finishing
    }

    fn seal_success(&self) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.reason.is_none() && inner.result.is_none() {
            inner.success_sealed = true;
            true
        } else {
            false
        }
    }

    fn publish_success(&self, summary: ConcurrentRunSummary) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        debug_assert!(inner.success_sealed && inner.reason.is_none());
        inner.state = ConcurrentRuntimeState::Finished;
        inner.result = Some(Ok(summary));
        self.changed.notify_all();
    }

    fn begin_aborting(&self) -> AbortReason {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.state = ConcurrentRuntimeState::Aborting;
        inner.reason.clone().expect("abort reason installed")
    }

    fn publish_abort(&self, reason: AbortReason) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.state = ConcurrentRuntimeState::Aborted;
        inner.result = Some(Err(reason));
        self.changed.notify_all();
    }

    fn node_inactive(&self, node_id: &NodeId) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active_nodes
            .remove(node_id);
    }

    fn lifecycle_enter(&self, node_id: NodeId) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .active_nodes
            .insert(node_id);
    }

    fn lifecycle_exit(&self, node_id: &NodeId) {
        self.node_inactive(node_id);
    }
}

#[derive(Default)]
struct LaunchGate {
    state: Mutex<Option<bool>>,
    changed: Condvar,
}

impl LaunchGate {
    fn wait(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.is_none() {
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        state.unwrap_or(false)
    }

    fn release(&self) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(true);
        self.changed.notify_all();
    }

    fn cancel(&self) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(false);
        self.changed.notify_all();
    }
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
    signal_queues: Arc<BTreeMap<EdgeId, crate::signal::SignalQueue>>,
    control: Arc<RuntimeControl>,
    gate: Arc<PrepareGate>,
    launch: Arc<LaunchGate>,
    options: RuntimeOptions,
    abort_diagnostics: Arc<Mutex<Vec<AbortHookDiagnostic>>>,
    event_bus: EventBus,
    resources: ResourceStore,
    transport: TransportControl,
}

struct InputEdge {
    port: PortName,
    queue: crate::EdgeQueue,
    signal_queue: crate::signal::SignalQueue,
}

struct OutputEdge {
    descriptor: EdgeDescriptor,
    queue: crate::EdgeQueue,
    signal_queue: crate::signal::SignalQueue,
    policy: Option<Box<dyn EdgePolicy>>,
}

struct OutputDispatch {
    descriptor: EdgeDescriptor,
    sender: mpsc::SyncSender<Dispatch>,
}

enum Dispatch {
    Frames(Vec<Frame>),
    Signals(Vec<SignalFrame>),
}

struct NodeCallOutput {
    emissions: Vec<NodeEmission>,
    signals: Vec<SignalFrame>,
}

struct NodeWorker {
    node_id: NodeId,
    node: Box<dyn Node>,
    incoming: Vec<InputEdge>,
    outgoing: Vec<OutputDispatch>,
    wake: Arc<QueueWake>,
    shared: Arc<WorkerShared>,
}

struct WorkerExit {
    node_id: NodeId,
    node: Box<dyn Node>,
    prepared: bool,
}

enum RuntimeExit {
    Node(WorkerExit),
    Edge(Box<OutputEdge>),
}

impl NodeWorker {
    fn cancelled_exit(self) -> WorkerExit {
        WorkerExit {
            node_id: self.node_id,
            node: self.node,
            prepared: false,
        }
    }

    fn run(mut self) -> WorkerExit {
        let prepared = match call_prepare(
            &mut *self.node,
            &self.node_id,
            &self.shared.graph,
            self.shared.options.emission_budget(),
        ) {
            Ok(()) => true,
            Err(reason) => {
                fail_graph(&self.shared, reason);
                false
            }
        };
        self.shared.gate.arrive_and_wait();
        if !self.shared.stop.is_cancelled() {
            self.shared.control.mark_running();
        }

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

        self.outgoing.clear();
        self.shared.control.node_inactive(&self.node_id);
        WorkerExit {
            node_id: self.node_id,
            node: self.node,
            prepared,
        }
    }

    fn run_source(&mut self) -> Result<(), AbortReason> {
        let output = call_process(
            &mut *self.node,
            &self.node_id,
            None,
            None,
            &self.shared.graph,
            self.shared.options.emission_budget(),
            !self.outgoing.is_empty(),
        )?;
        self.route(output)
    }

    fn run_consumer(&mut self) -> Result<(), AbortReason> {
        let mut cursor = 0usize;
        loop {
            let observed = self.wake.generation();
            let mut received_signal = None;
            for offset in 0..self.incoming.len() {
                let index = (cursor + offset) % self.incoming.len();
                if let Some(signal) = self.incoming[index].signal_queue.try_pop() {
                    received_signal = Some((index, signal));
                    cursor = (index + 1) % self.incoming.len();
                    break;
                }
            }
            if let Some((index, signal)) = received_signal {
                let input_port = self.incoming[index].port.clone();
                let output = call_signal(
                    &mut *self.node,
                    &self.node_id,
                    signal,
                    Some(input_port),
                    &self.shared.graph,
                    self.shared.options.emission_budget(),
                    !self.outgoing.is_empty(),
                )?;
                self.route(output)?;
                continue;
            }
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
                let is_sink = self
                    .shared
                    .graph
                    .node(&self.node_id)
                    .expect("validated node")
                    .descriptor()
                    .kind()
                    == NodeKind::Sink;
                if is_sink {
                    match self.shared.transport.should_deliver_to_sink(&frame) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(error) => {
                            let reason = error.to_string();
                            return Err(runtime_abort_details(
                                "VOXA-TRANSPORT-TURN-FRAME",
                                "invalid turn scope before Sink delivery",
                                Some(self.node_id.clone()),
                                [("reason", reason.as_str())],
                            ));
                        }
                    }
                }
                let input_port = self.incoming[index].port.clone();
                let output = call_process(
                    &mut *self.node,
                    &self.node_id,
                    Some(frame),
                    Some(input_port),
                    &self.shared.graph,
                    self.shared.options.emission_budget(),
                    !self.outgoing.is_empty(),
                )?;
                self.route(output)?;
                continue;
            }
            if self.incoming.iter().all(|edge| {
                edge.queue.is_closed_and_empty() && edge.signal_queue.is_closed_and_empty()
            }) {
                return Ok(());
            }
            self.wake.wait_for_change(observed);
        }
    }

    fn route(&mut self, output: NodeCallOutput) -> Result<(), AbortReason> {
        let descriptor = self
            .shared
            .graph
            .node(&self.node_id)
            .expect("validated node")
            .descriptor();
        let mut batches = (0..self.outgoing.len())
            .map(|_| Vec::new())
            .collect::<Vec<_>>();
        for emission in output.emissions {
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
            for (index, output) in self.outgoing.iter().enumerate() {
                if output.descriptor.from_output_port() == &output_port {
                    batches[index].push(frame.clone());
                }
            }
        }

        // Every edge receives its complete bounded callback batch before this
        // worker waits for a congested dispatcher. This prevents a full slow
        // branch from serially withholding the same callback from a bypass.
        let mut pending = Vec::new();
        for (index, batch) in batches.into_iter().enumerate() {
            if batch.is_empty() {
                continue;
            }
            match self.outgoing[index]
                .sender
                .try_send(Dispatch::Frames(batch))
            {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(Dispatch::Frames(batch))) => {
                    pending.push((index, batch))
                }
                Err(mpsc::TrySendError::Full(Dispatch::Signals(_))) => unreachable!("sent frames"),
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return self.dispatch_closed_abort();
                }
            }
        }
        for (index, batch) in pending {
            if self.outgoing[index]
                .sender
                .send(Dispatch::Frames(batch))
                .is_err()
            {
                return self.dispatch_closed_abort();
            }
        }
        if !output.signals.is_empty() {
            for outgoing in &self.outgoing {
                if outgoing
                    .sender
                    .send(Dispatch::Signals(output.signals.clone()))
                    .is_err()
                {
                    return self.dispatch_closed_abort();
                }
            }
        }
        Ok(())
    }

    fn dispatch_closed_abort(&self) -> Result<(), AbortReason> {
        if let Some(reason) = self.shared.control.reason() {
            Err(reason)
        } else {
            Err(runtime_abort(
                "VOXA-CONCURRENT-DISPATCH-CLOSED",
                "edge dispatcher exited while its producer was active",
                Some(self.node_id.clone()),
                AbortCategory::NodeError,
                AbortStage::Runtime,
            ))
        }
    }
}

fn run_dispatcher(
    mut output: OutputEdge,
    receiver: mpsc::Receiver<Dispatch>,
    shared: &WorkerShared,
) -> OutputEdge {
    'dispatch: for dispatch in receiver {
        match dispatch {
            Dispatch::Frames(batch) => {
                for frame in batch {
                    if let Err(reason) = apply_edge(&shared.graph, &mut output, frame) {
                        fail_graph(shared, reason);
                        break 'dispatch;
                    }
                }
            }
            Dispatch::Signals(signals) => {
                for signal in signals {
                    if let Err(reason) = apply_signal(&shared.graph, &mut output, signal) {
                        fail_graph(shared, reason);
                        break 'dispatch;
                    }
                }
            }
        }
    }
    output.queue.close(DrainMode::Drain);
    output.signal_queue.close(DrainMode::Drain);
    output
}

fn apply_signal(
    graph: &GraphDefinition,
    output: &mut OutputEdge,
    signal: SignalFrame,
) -> Result<(), AbortReason> {
    output.queue.record_signal();
    if let Some(policy) = output.policy.as_mut() {
        let snapshot = output.queue.snapshot();
        let context = EdgeContext::new(graph, &output.descriptor, &snapshot);
        match catch_unwind(AssertUnwindSafe(|| policy.on_signal(&signal, &context))) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                return Err(policy_error(
                    &output.descriptor,
                    &output.queue,
                    error,
                    "on_signal",
                ))
            }
            Err(payload) => {
                return Err(policy_panic(
                    &output.descriptor,
                    &output.queue,
                    payload,
                    "on_signal",
                ))
            }
        }
    }
    output.signal_queue.try_push(signal).map_err(|error| {
        let (code, message) = match error {
            SignalQueuePushError::Full => (
                "VOXA-SIGNAL-QUEUE-FULL",
                "bounded adjacent Signal queue is full",
            ),
            SignalQueuePushError::Closed => (
                "VOXA-SIGNAL-QUEUE-CLOSED",
                "adjacent Signal queue is closed",
            ),
        };
        edge_abort(&output.descriptor, code, message, "signal_queue")
    })
}

fn coordinate(
    shared: Arc<WorkerShared>,
    exits: mpsc::Receiver<RuntimeExit>,
    handles: Vec<thread::JoinHandle<()>>,
    worker_total: usize,
) {
    let mut nodes = BTreeMap::new();
    let mut resources = Vec::new();
    let mut prepared = BTreeSet::new();
    for exit in exits {
        match exit {
            RuntimeExit::Node(exit) => {
                if exit.prepared {
                    prepared.insert(exit.node_id.clone());
                }
                nodes.insert(exit.node_id, exit.node);
            }
            RuntimeExit::Edge(output) => resources.push(*output),
        }
    }
    for handle in handles {
        let _ = handle.join();
    }

    if shared.control.reason().is_none() && shared.control.begin_finishing() {
        for node_id in shared.graph.topological_order().iter().rev() {
            if let Some(node) = nodes.get_mut(node_id) {
                shared.control.lifecycle_enter(node_id.clone());
                if let Err(error) = call_finish(
                    &mut **node,
                    node_id,
                    &shared.graph,
                    shared.options.emission_budget(),
                ) {
                    shared.control.lifecycle_exit(node_id);
                    fail_graph(&shared, error);
                    break;
                }
                shared.control.lifecycle_exit(node_id);
            }
        }
        if shared.control.seal_success() {
            drop(nodes);
            drop(resources);
            let _ = shared.event_bus.stop(Duration::from_millis(100));
            shared.resources.stop();
            shared
                .control
                .publish_success(ConcurrentRunSummary { worker_total });
            return;
        }
    }

    let reason = shared.control.begin_aborting();
    let diagnostics = shared.abort_diagnostics.clone();
    for node_id in shared.graph.topological_order().iter().rev() {
        if !prepared.contains(node_id) {
            continue;
        }
        let config = shared.graph.node(node_id).expect("node").config().clone();
        let mut context = NodeContext::with_emission_limit(
            node_id.clone(),
            config,
            None,
            shared.options.emission_budget(),
        );
        if let Some(node) = nodes.get_mut(node_id) {
            shared.control.lifecycle_enter(node_id.clone());
            if let Err(payload) =
                catch_unwind(AssertUnwindSafe(|| node.on_abort(&reason, &mut context)))
            {
                diagnostics.lock().unwrap_or_else(|e| e.into_inner()).push(
                    AbortHookDiagnostic::new(node_id.clone(), panic_message(payload.as_ref())),
                );
            }
            shared.control.lifecycle_exit(node_id);
        }
    }

    drop(nodes);
    drop(resources);
    let _ = shared.event_bus.stop(Duration::from_millis(100));
    shared.resources.stop();
    shared.control.publish_abort(reason);
}

fn call_prepare(
    node: &mut dyn Node,
    node_id: &NodeId,
    graph: &GraphDefinition,
    emission_budget: usize,
) -> Result<(), AbortReason> {
    let config = graph.node(node_id).expect("node").config().clone();
    let mut context =
        NodeContext::with_emission_limit(node_id.clone(), config, None, emission_budget);
    let outcome = catch_unwind(AssertUnwindSafe(|| node.on_prepare(&mut context)));
    if context.emission_overflowed() {
        return Err(emission_limit_abort(node_id, emission_budget));
    }
    match outcome {
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
    emission_budget: usize,
    has_signal_routes: bool,
) -> Result<NodeCallOutput, AbortReason> {
    let config = graph.node(node_id).expect("node").config().clone();
    let mut context = NodeContext::with_routing_limits(
        node_id.clone(),
        config,
        input_port,
        emission_budget,
        has_signal_routes,
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| node.on_process(input, &mut context)));
    if context.emission_overflowed() {
        return Err(emission_limit_abort(node_id, emission_budget));
    }
    match outcome {
        Ok(Ok(())) => Ok(NodeCallOutput {
            emissions: context.take_emissions(),
            signals: context.take_signals(),
        }),
        Ok(Err(error)) => Err(node_error(error, node_id, AbortStage::Process)),
        Err(payload) => Err(panic_abort(
            payload,
            Some(node_id),
            AbortStage::Process,
            None,
        )),
    }
}

fn call_signal(
    node: &mut dyn Node,
    node_id: &NodeId,
    signal: SignalFrame,
    input_port: Option<PortName>,
    graph: &GraphDefinition,
    emission_budget: usize,
    has_signal_routes: bool,
) -> Result<NodeCallOutput, AbortReason> {
    let config = graph.node(node_id).expect("node").config().clone();
    let mut context = NodeContext::with_routing_limits(
        node_id.clone(),
        config,
        input_port,
        emission_budget,
        has_signal_routes,
    );
    let outcome = catch_unwind(AssertUnwindSafe(|| node.on_signal(signal, &mut context)));
    if context.emission_overflowed() {
        return Err(emission_limit_abort(node_id, emission_budget));
    }
    match outcome {
        Ok(Ok(())) => Ok(NodeCallOutput {
            emissions: context.take_emissions(),
            signals: context.take_signals(),
        }),
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
    emission_budget: usize,
) -> Result<(), AbortReason> {
    let config = graph.node(node_id).expect("node").config().clone();
    let mut context =
        NodeContext::with_emission_limit(node_id.clone(), config, None, emission_budget);
    let outcome = catch_unwind(AssertUnwindSafe(|| node.on_finish(&mut context)));
    if context.emission_overflowed() {
        return Err(emission_limit_abort(node_id, emission_budget));
    }
    match outcome {
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
    if shared.control.request_abort(reason) {
        shared.stop.cancel();
        shared.event_bus.request_stop();
        shared.resources.seal();
        close_all(&shared.queues, shared.options.failure_mode);
        close_all_signals(&shared.signal_queues, shared.options.failure_mode);
    }
}

fn cleanup_failed_start(shared: &WorkerShared, handles: Vec<thread::JoinHandle<()>>) {
    shared.launch.cancel();
    shared.stop.cancel();
    close_all(&shared.queues, DrainMode::Discard);
    close_all_signals(&shared.signal_queues, DrainMode::Discard);
    for handle in handles {
        let _ = handle.join();
    }
}

fn emission_limit_abort(node_id: &NodeId, limit: usize) -> AbortReason {
    let limit = limit.to_string();
    runtime_abort_details(
        "VOXA-CONCURRENT-EMISSION-LIMIT",
        "node lifecycle call exceeded its bounded emission budget",
        Some(node_id.clone()),
        [("emission_limit", limit.as_str())],
    )
}

fn close_all(queues: &BTreeMap<EdgeId, crate::EdgeQueue>, mode: DrainMode) {
    for queue in queues.values() {
        queue.close(mode);
    }
}

fn close_all_signals(queues: &BTreeMap<EdgeId, crate::signal::SignalQueue>, mode: DrainMode) {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::{ConfigSchema, GraphBuilder, LifecycleCapabilities, NodeDescriptor, NodeTypeName};

    struct FailingSpawner {
        fail_at: usize,
        calls: AtomicUsize,
    }

    impl ThreadSpawner for FailingSpawner {
        fn spawn(
            &self,
            name: String,
            task: Box<dyn FnOnce() + Send + 'static>,
        ) -> io::Result<thread::JoinHandle<()>> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == self.fail_at {
                return Err(io::Error::other("injected spawn failure"));
            }
            thread::Builder::new().name(name).spawn(task)
        }
    }

    struct ProbeSource(Arc<AtomicBool>);

    impl Node for ProbeSource {
        fn on_process(&mut self, _: Option<Frame>, _: &mut NodeContext) -> voxa_types::Result<()> {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn coordinator_spawn_failure_cancels_and_joins_started_workers() {
        let node_id = NodeId::new("source").unwrap();
        let descriptor = NodeDescriptor::new(
            node_id.clone(),
            NodeTypeName::new("test.source").unwrap(),
            NodeKind::Source,
            Vec::new(),
            ConfigSchema::empty(),
            LifecycleCapabilities::default(),
        );
        let mut builder = GraphBuilder::new();
        builder.add_node(descriptor).unwrap();
        let called = Arc::new(AtomicBool::new(false));
        let mut nodes: NodeInstances = BTreeMap::new();
        nodes.insert(node_id, Box::new(ProbeSource(called.clone())));
        let runtime = ConcurrentRuntime::new(
            builder.build().unwrap(),
            nodes,
            EdgePolicies::new(),
            RuntimeOptions::default(),
        )
        .unwrap();
        let error = match runtime.start_with_spawner(&FailingSpawner {
            fail_at: 1,
            calls: AtomicUsize::new(0),
        }) {
            Ok(_) => panic!("injected coordinator spawn unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.role(), RuntimeThreadRole::Coordinator);
        assert!(!called.load(Ordering::SeqCst));
    }
}
