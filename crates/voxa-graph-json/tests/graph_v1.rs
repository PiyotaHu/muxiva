use std::{collections::BTreeMap, sync::Arc};

use voxa_core::{
    ConfigSchema, EdgePolicies, GraphRunner, LifecycleCapabilities, Node, NodeContext,
    NodeDescriptor, NodeFactory, NodeFactoryError, NodeFactoryVersion, NodeInstances, NodeKind,
    NodeLanguage, NodeRegistration, NodeRegistry, NodeTypeName,
};
use voxa_graph_json::{
    builtin_node_catalog, builtin_registry, compile, compile_with_registry, parse, GRAPH_V1_SCHEMA,
};
use voxa_types::{Frame, NodeId, Value};

const VALID: &str = include_str!("../../../examples/graphs/text-uppercase.v1.json");

#[test]
fn graph_v1_compiles_with_exact_factories_and_preserves_config() {
    let graph = compile(&parse(VALID).unwrap()).unwrap();
    assert_eq!(graph.graph_id().as_str(), "text-uppercase");
    assert_eq!(graph.nodes().len(), 3);
    assert_eq!(graph.edges().len(), 2);

    let source = graph
        .nodes()
        .iter()
        .find(|node| node.descriptor().node_id().as_str() == "source")
        .unwrap();
    assert_eq!(
        source.config().get("text"),
        Some(&Value::String("hello".into()))
    );
    let factory = source.factory().expect("JSON nodes select exact factories");
    assert_eq!(factory.language(), NodeLanguage::Rust);
    assert_eq!(factory.version().as_str(), "1.0.0");
}

#[test]
fn unknown_registration_invalid_config_and_zero_queue_are_precise() {
    let invalid = VALID
        .replace(
            "\"factory_version\":\"1.0.0\"",
            "\"factory_version\":\"9.0.0\"",
        )
        .replace("\"capacity\":32", "\"capacity\":0");
    let errors = compile(&parse(&invalid).unwrap()).unwrap_err();
    assert!(errors
        .iter()
        .any(|error| error.code == "VOXA-GRAPH-REGISTRY"));
    assert!(errors
        .iter()
        .any(|error| error.pointer.starts_with("/edges/")));

    let invalid_config = VALID.replacen("{\"text\":\"hello\"}", "{\"text\":42}", 1);
    let errors = compile(&parse(&invalid_config).unwrap()).unwrap_err();
    assert!(errors.iter().any(|error| {
        error.code == "VOXA-GRAPH-CONFIG" && error.pointer == "/nodes/0/node_config"
    }));
}

#[test]
fn factory_version_is_required_and_language_has_no_implicit_default() {
    for missing in [
        VALID.replacen("\"factory_version\":\"1.0.0\",", "", 1),
        VALID.replacen("\"language\":\"rust\",", "", 1),
        VALID.replacen(",\"node_config\":{\"text\":\"hello\"}", "", 1),
    ] {
        let errors = parse(&missing).unwrap_err();
        assert_eq!(errors[0].code, "VOXA-GRAPH-JSON");
    }
}

#[test]
fn every_core_frame_spelling_reaches_exact_port_type_validation() {
    for frame_type in ["audio", "video", "byte", "signal", "event"] {
        let changed = VALID.replacen(
            "\"frame_type\":\"text\"",
            &format!("\"frame_type\":\"{frame_type}\""),
            1,
        );
        let errors = compile(&parse(&changed).unwrap()).unwrap_err();
        assert!(errors.iter().any(|error| {
            error.code == "VOXA-GRAPH-EDGE"
                && error.message.contains("declares")
                && !error.message.contains("unknown frame_type")
        }));
    }
}

#[test]
fn schema_is_machine_readable_and_declares_exact_factory_fields() {
    let value: serde_json::Value = serde_json::from_str(GRAPH_V1_SCHEMA).unwrap();
    assert_eq!(value["properties"]["version"]["const"], "voxa.graph/v1");
    assert!(value["$defs"]["node"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|field| field == "factory_version"));
    assert_eq!(
        value["$defs"]["queue_policy"]["properties"]["capacity"]["minimum"],
        1
    );
}

#[test]
fn studio_catalog_is_derived_from_registry_descriptors_and_schemas() {
    let catalog = builtin_node_catalog();
    assert_eq!(catalog.len(), 3);
    let json = serde_json::to_value(catalog).unwrap();
    let source = json
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["node_type"] == "builtin.text_source")
        .unwrap();
    assert_eq!(source["factory_version"], "1.0.0");
    assert_eq!(source["config_schema"]["required"][0], "text");
    assert_eq!(source["ports"][0]["name"], "text_out");
}

#[test]
fn compiled_builtins_materialize_and_run_through_the_same_registry() {
    let graph = compile(&parse(VALID).unwrap()).unwrap();
    let registry = builtin_registry();
    let mut instances: NodeInstances = BTreeMap::new();
    for definition in graph.nodes() {
        let selection = definition.factory().unwrap();
        let descriptor = definition.descriptor();
        instances.insert(
            descriptor.node_id().clone(),
            registry
                .create(
                    descriptor.node_type(),
                    selection.language(),
                    selection.version(),
                    descriptor.node_id().clone(),
                    definition.config(),
                )
                .unwrap(),
        );
    }
    let mut runner = GraphRunner::new(&graph, instances, EdgePolicies::new()).unwrap();
    runner.run().unwrap();
}

struct CustomFactory;

impl NodeFactory for CustomFactory {
    fn validate_config(&self, config: &voxa_core::ConfigMap) -> Result<(), NodeFactoryError> {
        match config.get("nested") {
            Some(Value::Map(values)) if values.get("enabled") == Some(&Value::Bool(true)) => Ok(()),
            _ => Err(NodeFactoryError::new(
                "CUSTOM-CONFIG",
                "nested.enabled must be true",
            )),
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &voxa_core::ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(NoopSource))
    }
}

struct NoopSource;

impl Node for NoopSource {
    fn on_process(
        &mut self,
        _input: Option<Frame>,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        Ok(())
    }
}

#[test]
fn custom_registry_is_the_only_source_needed_by_the_compiler() {
    let node_type = NodeTypeName::new("custom.source").unwrap();
    let version = NodeFactoryVersion::new("2026.1").unwrap();
    let mut registry = NodeRegistry::default();
    registry
        .register(NodeRegistration::new(
            NodeLanguage::Python,
            NodeDescriptor::new(
                NodeId::new("template-custom").unwrap(),
                node_type,
                NodeKind::Source,
                Vec::new(),
                ConfigSchema::empty(),
                LifecycleCapabilities::default(),
            ),
            version,
            Arc::new(CustomFactory),
        ))
        .unwrap();
    let document = parse(
        r#"{"version":"voxa.graph/v1","graph_id":"custom","nodes":[{"id":"source","node_type":"custom.source","language":"python","factory_version":"2026.1","node_config":{"nested":{"enabled":true}}}],"edges":[]}"#,
    )
    .unwrap();
    let graph = compile_with_registry(&document, &registry).unwrap();
    assert_eq!(
        graph.nodes()[0].factory().unwrap().language(),
        NodeLanguage::Python
    );
    assert!(matches!(
        graph.nodes()[0].config().get("nested"),
        Some(Value::Map(values)) if values.get("enabled") == Some(&Value::Bool(true))
    ));
}
