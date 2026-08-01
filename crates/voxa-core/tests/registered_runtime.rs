use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use voxa_core::{
    materialize_registered_nodes, start_registered_runtime, AbortCategory, ConfigMap, ConfigSchema,
    EdgePolicies, GraphBuilder, GraphMaterializationError, LifecycleCapabilities, Node,
    NodeContext, NodeDescriptor, NodeFactory, NodeFactoryError, NodeFactorySelection,
    NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration, NodeRegistry, NodeTypeName,
    RuntimeOptions, RuntimeWaitError,
};
use voxa_types::{ErrorCategory, Frame, GraphId, NodeId, VoxaError};

const NODE_TYPE: &str = "test.registered-source";
const VERSION: &str = "1.0.0";

#[derive(Clone, Copy)]
enum Behavior {
    Success,
    Abort,
    Slow,
}

struct TestFactory {
    behavior: Behavior,
    calls: Arc<AtomicUsize>,
    fail_create: bool,
}

impl NodeFactory for TestFactory {
    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        if self.fail_create {
            return Err(NodeFactoryError::new(
                "TEST-CREATE",
                "requested Factory creation failure",
            ));
        }
        Ok(Box::new(TestSource {
            behavior: self.behavior,
            calls: self.calls.clone(),
        }))
    }
}

struct TestSource {
    behavior: Behavior,
    calls: Arc<AtomicUsize>,
}

impl Node for TestSource {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        assert!(input.is_none());
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.behavior {
            Behavior::Success => Ok(()),
            Behavior::Abort => Err(VoxaError::new(
                ErrorCategory::External,
                "VOXA-TEST-ABORT",
                "registered node aborted",
            )),
            Behavior::Slow => {
                thread::sleep(Duration::from_millis(50));
                Ok(())
            }
        }
    }
}

fn node_id() -> NodeId {
    NodeId::new("source").unwrap()
}

fn node_type() -> NodeTypeName {
    NodeTypeName::new(NODE_TYPE).unwrap()
}

fn version() -> NodeFactoryVersion {
    NodeFactoryVersion::new(VERSION).unwrap()
}

fn graph(with_selection: bool) -> voxa_core::GraphDefinition {
    let id = node_id();
    let mut builder = GraphBuilder::with_graph_id(GraphId::new("registered-test").unwrap());
    builder
        .add_node(NodeDescriptor::new(
            id.clone(),
            node_type(),
            NodeKind::Source,
            Vec::new(),
            ConfigSchema::empty(),
            LifecycleCapabilities::default(),
        ))
        .unwrap();
    if with_selection {
        builder
            .set_factory(
                &id,
                NodeFactorySelection::new(NodeLanguage::Rust, version()),
            )
            .unwrap();
    }
    builder.build().unwrap()
}

fn registry(behavior: Behavior, calls: Arc<AtomicUsize>, fail_create: bool) -> NodeRegistry {
    let template = NodeId::new("template-source").unwrap();
    let mut registry = NodeRegistry::default();
    registry
        .register(NodeRegistration::new(
            NodeLanguage::Rust,
            NodeDescriptor::new(
                template,
                node_type(),
                NodeKind::Source,
                Vec::new(),
                ConfigSchema::empty(),
                LifecycleCapabilities::default(),
            ),
            version(),
            Arc::new(TestFactory {
                behavior,
                calls,
                fail_create,
            }),
        ))
        .unwrap();
    registry
}

#[test]
fn exact_registered_factory_materializes_and_runs_concurrently() {
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = registry(Behavior::Success, calls.clone(), false);
    let runtime = start_registered_runtime(
        graph(true),
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap();
    let summary = runtime.wait(Duration::from_secs(1)).unwrap();
    assert_eq!(summary.worker_total(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let metrics = runtime.node_metrics(&node_id()).unwrap();
    assert_eq!(metrics.prepare_total(), 1);
    assert_eq!(metrics.process_total(), 1);
    assert_eq!(metrics.finish_total(), 1);
    assert_eq!(metrics.error_total(), 0);
    assert!(metrics.callback_duration_ns() >= metrics.max_callback_duration_ns());
}

#[test]
fn missing_selection_and_factory_creation_failure_are_pre_start_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let working_registry = registry(Behavior::Success, calls, false);
    assert!(matches!(
        materialize_registered_nodes(&graph(false), &working_registry),
        Err(GraphMaterializationError::MissingFactorySelection { .. })
    ));

    let failing = registry(Behavior::Success, Arc::new(AtomicUsize::new(0)), true);
    let error = materialize_registered_nodes(&graph(true), &failing)
        .err()
        .expect("Factory creation must fail");
    assert!(matches!(
        error,
        GraphMaterializationError::NodeCreation { source, .. }
            if source.to_string().contains("TEST-CREATE")
    ));
}

#[test]
fn registered_node_abort_reaches_the_runtime_terminal_result() {
    let registry = registry(Behavior::Abort, Arc::new(AtomicUsize::new(0)), false);
    let runtime = start_registered_runtime(
        graph(true),
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap();
    let RuntimeWaitError::Aborted(reason) = runtime.wait(Duration::from_secs(1)).unwrap_err()
    else {
        panic!("expected registered node abort");
    };
    assert_eq!(reason.category(), AbortCategory::ExternalSdkError);
    assert_eq!(reason.root().code(), "VOXA-TEST-ABORT");
    let metrics = runtime.node_metrics(&node_id()).unwrap();
    assert_eq!(metrics.process_total(), 1);
    assert_eq!(metrics.abort_total(), 1);
    assert_eq!(metrics.error_total(), 1);
    assert_eq!(metrics.panic_total(), 0);
}

#[test]
fn registered_runtime_timeout_reports_live_nodes_then_stops_boundedly() {
    let registry = registry(Behavior::Slow, Arc::new(AtomicUsize::new(0)), false);
    let runtime = start_registered_runtime(
        graph(true),
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap();
    let RuntimeWaitError::Timeout(diagnostics) =
        runtime.wait(Duration::from_millis(1)).unwrap_err()
    else {
        panic!("expected bounded wait timeout");
    };
    assert_eq!(diagnostics.active_nodes(), &[node_id()]);
    assert!(runtime.stop());
    assert!(matches!(
        runtime.wait(Duration::from_secs(1)),
        Err(RuntimeWaitError::Aborted(reason)) if reason.category() == AbortCategory::Cancelled
    ));
}
