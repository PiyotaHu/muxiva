use std::{collections::BTreeMap, sync::Arc, time::Duration};

use muxiva_core::{
    ConcurrentRuntime, ConfigSchema, EdgePolicies, GraphRunner, LifecycleCapabilities, Node,
    NodeContext, NodeDescriptor, NodeFactory, NodeFactoryError, NodeFactoryVersion, NodeInstances,
    NodeKind, NodeLanguage, NodeRegistration, NodeRegistry, NodeTypeName, RuntimeOptions,
};
use muxiva_graph_json::{
    builtin_node_catalog, builtin_registry, compile, compile_with_registry, parse, GRAPH_V1_SCHEMA,
};
use muxiva_types::{EdgeId, Frame, NodeId, Value};

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
        .any(|error| error.code == "MUXIVA-GRAPH-REGISTRY"));
    assert!(errors
        .iter()
        .any(|error| error.pointer.starts_with("/edges/")));

    let invalid_config = VALID.replacen("{\"text\":\"hello\"}", "{\"text\":42}", 1);
    let errors = compile(&parse(&invalid_config).unwrap()).unwrap_err();
    assert!(errors.iter().any(|error| {
        error.code == "MUXIVA-GRAPH-CONFIG" && error.pointer == "/nodes/0/node_config"
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
        assert_eq!(errors[0].code, "MUXIVA-GRAPH-JSON");
    }
}

#[test]
fn legacy_official_node_names_and_agora_clock_are_migrated() {
    let legacy = r#"{
        "version":"muxiva.graph/v1",
        "graph_id":"legacy-voice",
        "nodes":[
          {"id":"clock","node_type":"builtin.interval_tick","language":"rust","factory_version":"1.0.0","node_config":{"interval_ms":20}},
          {"id":"source","node_type":"provider.agora.audio_source","language":"cpp","factory_version":"1.0.0","node_config":{}},
          {"id":"resampler","node_type":"builtin.audio_resample","language":"rust","factory_version":"1.0.0","node_config":{"sample_rate_hz":16000}},
          {"id":"model","node_type":"provider.qwen.audio_realtime","language":"python","factory_version":"1.0.0","node_config":{}}
        ],
        "edges":[
          {"id":"tick","from":{"node_id":"clock","port":"tick_out"},"to":{"node_id":"source","port":"tick_in"},"frame_type":"event","queue_policy":{"capacity":1,"overflow":"drop_oldest"}},
          {"id":"audio","from":{"node_id":"source","port":"audio_out"},"to":{"node_id":"resampler","port":"audio_in"},"frame_type":"audio","queue_policy":{"capacity":8,"overflow":"block"}}
        ]
    }"#;

    let graph = parse(legacy).unwrap();
    assert!(!graph.nodes.iter().any(|node| node.id == "clock"));
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.nodes[0].node_type, "agora.audio_source");
    assert_eq!(graph.nodes[0].factory_version, "1.1.0");
    assert_eq!(graph.nodes[1].node_type, "builtin.audio_resampler");
    assert_eq!(graph.nodes[2].node_type, "qwen.audio_realtime");
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
            error.code == "MUXIVA-GRAPH-EDGE"
                && error.message.contains("declares")
                && !error.message.contains("unknown frame_type")
        }));
    }
}

#[test]
fn schema_is_machine_readable_and_declares_exact_factory_fields() {
    let value: serde_json::Value = serde_json::from_str(GRAPH_V1_SCHEMA).unwrap();
    assert_eq!(value["properties"]["version"]["const"], "muxiva.graph/v1");
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
    assert_eq!(catalog.len(), 19);
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
    assert!(json
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| { entry["node_type"] != "builtin.client_event_encoder" }));
    let speech_formatter = json
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["node_type"] == "builtin.speech_formatter")
        .unwrap();
    assert_eq!(speech_formatter["capability"], "text.speech_format");
    assert_eq!(
        speech_formatter["config_schema"]["properties"]["strip_urls"]["default"],
        true
    );
    assert_eq!(
        speech_formatter["config_schema"]["properties"]["suppressed_parenthetical_terms"]
            ["default"],
        serde_json::json!([])
    );
    assert!(speech_formatter["ports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|port| port["name"] == "event_in"));
    let voice_turn = json
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["node_type"] == "builtin.voice_turn_controller")
        .unwrap();
    assert_eq!(voice_turn["capability"], "conversation.turn_control");
    assert_eq!(voice_turn["ports"][0]["name"], "transcript_in");
    let canonical_signal = voice_turn["ports"]
        .as_array()
        .unwrap()
        .iter()
        .find(|port| port["name"] == "signal_out")
        .unwrap();
    assert_eq!(
        canonical_signal["schema"]["signal_names"][0],
        "muxiva.turn.cancelled"
    );
    assert_eq!(
        voice_turn["config_schema"]["properties"]["ignore_filler_utterances"]
            ["default"],
        true
    );
    assert_eq!(
        voice_turn["config_schema"]["properties"]["early_cancel_preview_hits"]
            ["default"],
        2
    );
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

#[test]
fn concurrent_voice_turn_controller_emits_exactly_one_canonical_cancel() {
    let document = parse(
        r#"{
          "version":"muxiva.graph/v1",
          "graph_id":"voice-turn-contract",
          "nodes":[
            {"id":"source","node_type":"builtin.text_source","language":"rust","factory_version":"1.0.0","node_config":{"text":"榴莲为什么这么臭"}},
            {"id":"turn","node_type":"builtin.voice_turn_controller","language":"rust","factory_version":"1.0.0","node_config":{"ignore_filler_utterances":true,"minimum_utterance_characters":3,"short_utterance_allowlist":["闭嘴"],"ignored_utterances":["嗯","咳嗽声"]}},
            {"id":"prompt-sink","node_type":"builtin.text_sink","language":"rust","factory_version":"1.0.0","node_config":{}},
            {"id":"cancel-sink","node_type":"builtin.text_cancellation_gate","language":"rust","factory_version":"1.0.0","node_config":{}}
          ],
          "edges":[
            {"id":"transcript","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"turn","port":"transcript_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}},
            {"id":"prompt","from":{"node_id":"turn","port":"prompt_out"},"to":{"node_id":"prompt-sink","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}},
            {"id":"cancel","from":{"node_id":"turn","port":"signal_out"},"to":{"node_id":"cancel-sink","port":"signal_in"},"frame_type":"signal","queue_policy":{"capacity":8,"overflow":"block"}}
          ]
        }"#,
    )
    .unwrap();
    let graph = compile(&document).unwrap();
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
    let runtime = ConcurrentRuntime::new(
        graph,
        instances,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .unwrap()
    .start()
    .unwrap();
    runtime.wait(Duration::from_secs(2)).unwrap();
    let metrics = runtime
        .signal_metrics(&EdgeId::new("cancel").unwrap())
        .unwrap();
    assert_eq!(metrics.enqueue_total, 1);
    assert_eq!(metrics.dequeue_total, 1);
}

struct CustomFactory;

impl NodeFactory for CustomFactory {
    fn validate_config(&self, config: &muxiva_core::ConfigMap) -> Result<(), NodeFactoryError> {
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
        _config: &muxiva_core::ConfigMap,
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
    ) -> muxiva_types::Result<()> {
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
        r#"{"version":"muxiva.graph/v1","graph_id":"custom","nodes":[{"id":"source","node_type":"custom.source","language":"python","factory_version":"2026.1","node_config":{"nested":{"enabled":true}}}],"edges":[]}"#,
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
