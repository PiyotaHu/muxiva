use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use voxa_core::{
    ConfigKey, ConfigMap, ConfigSchema, EdgePolicies, GraphBuildError, GraphBuilder, GraphRunner,
    LifecycleCapabilities, Node, NodeContext, NodeCreateError, NodeCreationStage, NodeDescriptor,
    NodeFactory, NodeFactoryError, NodeFactoryVersion, NodeInstances, NodeKind, NodeLanguage,
    NodeRegistration, NodeRegistry, NodeTypeName, PortDescriptor, PortDirection, PortName,
    RegistryError,
};
use voxa_types::{Frame, FrameType, NodeId, Value};

fn node_id(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn node_type() -> NodeTypeName {
    NodeTypeName::new("test.counting-source").unwrap()
}

fn version(value: &str) -> NodeFactoryVersion {
    NodeFactoryVersion::new(value).unwrap()
}

fn config(enabled: bool) -> ConfigMap {
    ConfigMap::try_from_iter([(ConfigKey::new("enabled").unwrap(), Value::Bool(enabled))]).unwrap()
}

fn descriptor() -> NodeDescriptor {
    let template_id = node_id("registry-template");
    NodeDescriptor::new(
        template_id.clone(),
        node_type(),
        NodeKind::Source,
        vec![PortDescriptor::new(
            template_id,
            PortName::new("output").unwrap(),
            PortDirection::Output,
            FrameType::Text,
        )],
        ConfigSchema::empty(),
        LifecycleCapabilities::default(),
    )
}

struct CountingFactory {
    creates: Arc<AtomicUsize>,
    processes: Arc<AtomicUsize>,
    panic_in_validate: bool,
    panic_in_create: bool,
}

impl NodeFactory for CountingFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        assert!(!self.panic_in_validate, "validation panic requested");
        match config.get("enabled") {
            Some(Value::Bool(true)) => Ok(()),
            _ => Err(NodeFactoryError::new(
                "TEST-CONFIG-ENABLED",
                "enabled must be true",
            )),
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        assert!(!self.panic_in_create, "creation panic requested");
        self.creates.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(CountingNode {
            processes: self.processes.clone(),
        }))
    }
}

struct CountingNode {
    processes: Arc<AtomicUsize>,
}

impl Node for CountingNode {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        assert!(input.is_none());
        self.processes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn registration(
    factory_version: &str,
    creates: Arc<AtomicUsize>,
    processes: Arc<AtomicUsize>,
) -> NodeRegistration {
    NodeRegistration::new(
        NodeLanguage::Rust,
        descriptor(),
        version(factory_version),
        Arc::new(CountingFactory {
            creates,
            processes,
            panic_in_validate: false,
            panic_in_create: false,
        }),
    )
}

#[test]
fn exact_versions_coexist_and_duplicates_are_rejected() {
    let mut registry = NodeRegistry::default();
    let creates = Arc::new(AtomicUsize::new(0));
    let processes = Arc::new(AtomicUsize::new(0));
    registry
        .register(registration("1.0.0", creates.clone(), processes.clone()))
        .unwrap();
    registry
        .register(registration("2.0.0", creates.clone(), processes.clone()))
        .unwrap();

    assert_eq!(registry.entries().count(), 2);
    assert_eq!(
        registry
            .resolve(&node_type(), NodeLanguage::Rust, &version("2.0.0"))
            .unwrap()
            .version()
            .as_str(),
        "2.0.0"
    );
    assert!(matches!(
        registry.register(registration("1.0.0", creates, processes)),
        Err(RegistryError::DuplicateNode { version, .. }) if version.as_str() == "1.0.0"
    ));
}

#[test]
fn registration_rebinds_descriptor_and_creates_a_runnable_node() {
    let creates = Arc::new(AtomicUsize::new(0));
    let processes = Arc::new(AtomicUsize::new(0));
    let mut registry = NodeRegistry::default();
    registry
        .register(registration("1.0.0", creates.clone(), processes.clone()))
        .unwrap();

    let actual_id = node_id("source-1");
    let registration = registry
        .resolve(&node_type(), NodeLanguage::Rust, &version("1.0.0"))
        .unwrap();
    let actual_descriptor = registration.descriptor_for(actual_id.clone());
    assert_eq!(actual_descriptor.node_id(), &actual_id);
    assert_eq!(actual_descriptor.ports()[0].node_id(), &actual_id);

    let node_config = config(true);
    let mut builder = GraphBuilder::new();
    builder.add_node(actual_descriptor).unwrap();
    builder.set_config(&actual_id, node_config.clone()).unwrap();
    let graph = builder.build().unwrap();

    let instance = registry
        .create(
            &node_type(),
            NodeLanguage::Rust,
            &version("1.0.0"),
            actual_id.clone(),
            &node_config,
        )
        .unwrap();
    let mut nodes: NodeInstances = BTreeMap::new();
    nodes.insert(actual_id, instance);
    let mut runner = GraphRunner::new(&graph, nodes, EdgePolicies::new()).unwrap();
    runner.run().unwrap();

    assert_eq!(creates.load(Ordering::SeqCst), 1);
    assert_eq!(processes.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_config_is_rejected_before_factory_creation() {
    let creates = Arc::new(AtomicUsize::new(0));
    let mut registry = NodeRegistry::default();
    registry
        .register(registration(
            "1.0.0",
            creates.clone(),
            Arc::new(AtomicUsize::new(0)),
        ))
        .unwrap();

    let error = registry
        .create(
            &node_type(),
            NodeLanguage::Rust,
            &version("1.0.0"),
            node_id("source-1"),
            &config(false),
        )
        .err()
        .expect("invalid config must fail");
    assert!(matches!(
        error,
        NodeCreateError::Factory {
            stage: NodeCreationStage::ValidateConfig,
            source,
            ..
        } if source.code() == "TEST-CONFIG-ENABLED"
    ));
    assert_eq!(creates.load(Ordering::SeqCst), 0);
}

#[test]
fn invalid_descriptor_is_rejected_before_it_enters_the_registry() {
    let invalid = NodeDescriptor::new(
        node_id("invalid-template"),
        node_type(),
        NodeKind::Source,
        Vec::new(),
        ConfigSchema::empty(),
        LifecycleCapabilities::new(false, false, false, false),
    );
    let factory = CountingFactory {
        creates: Arc::new(AtomicUsize::new(0)),
        processes: Arc::new(AtomicUsize::new(0)),
        panic_in_validate: false,
        panic_in_create: false,
    };
    let mut registry = NodeRegistry::default();
    let error = registry
        .register(NodeRegistration::new(
            NodeLanguage::Rust,
            invalid,
            version("1.0.0"),
            Arc::new(factory),
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        RegistryError::InvalidNodeDescriptor { source, .. }
            if matches!(source.as_ref(), GraphBuildError::ProcessCapabilityMissing { .. })
    ));
    assert_eq!(registry.entries().count(), 0);
}

#[test]
fn factory_panics_are_contained_at_both_creation_stages() {
    for (panic_in_validate, panic_in_create, expected_stage) in [
        (true, false, NodeCreationStage::ValidateConfig),
        (false, true, NodeCreationStage::Create),
    ] {
        let mut registry = NodeRegistry::default();
        registry
            .register(NodeRegistration::new(
                NodeLanguage::Rust,
                descriptor(),
                version("1.0.0"),
                Arc::new(CountingFactory {
                    creates: Arc::new(AtomicUsize::new(0)),
                    processes: Arc::new(AtomicUsize::new(0)),
                    panic_in_validate,
                    panic_in_create,
                }),
            ))
            .unwrap();

        let error = registry
            .create(
                &node_type(),
                NodeLanguage::Rust,
                &version("1.0.0"),
                node_id("source-1"),
                &config(true),
            )
            .err()
            .expect("factory panic must be contained");
        assert!(matches!(
            error,
            NodeCreateError::FactoryPanicked { stage, .. } if stage == expected_stage
        ));
    }
}

#[test]
fn factory_version_rejects_ambiguous_or_unbounded_identifiers() {
    for invalid in ["", " 1.0.0", "1/0", &"v".repeat(65)] {
        assert!(NodeFactoryVersion::new(invalid).is_err(), "{invalid:?}");
    }
    assert_eq!(version("1.0.0-alpha+rust").as_str(), "1.0.0-alpha+rust");
}

#[test]
fn registry_is_safe_to_share_between_compiler_and_runtime_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NodeRegistry>();
    assert_send_sync::<NodeRegistration>();
}
