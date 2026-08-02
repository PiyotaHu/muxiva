//! Strict Graph v1 configuration parser and Registry-driven compiler.

mod builtins;

use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroUsize};
use voxa_core::{
    ConfigKey, ConfigMap, EdgeDescriptor, EnabledCondition, GraphBuilder, GraphDefinition,
    NodeFactorySelection, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistry, PortDirection,
    QueueOverflowPolicy, QueuePolicy, TransformPolicy, ValidationPolicy, VisibilityDescriptor,
};
use voxa_types::{EdgeId, FiniteF64, FrameType, GraphId, NodeId, Value, ValueMap};

pub use builtins::{
    BUILTIN_FACTORY_VERSION, DEMO_CONTEXT_FUSION, DEMO_MICROPHONE, DEMO_NEURAL_TTS,
    DEMO_REASONING_LLM, DEMO_SPEAKER, DEMO_STREAMING_ASR, DEMO_VOICE_ACTIVITY, STDOUT_TEXT_SINK,
    TEXT_SINK, TEXT_SOURCE, UPPERCASE,
};

pub const GRAPH_V1_SCHEMA: &str = include_str!("../schema/graph-v1.schema.json");
pub const MAX_DOCUMENT_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphDocument {
    pub version: String,
    pub graph_id: String,
    pub nodes: Vec<NodeDocument>,
    pub edges: Vec<EdgeDocument>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDocument {
    pub id: String,
    pub node_type: String,
    pub language: String,
    pub factory_version: String,
    pub node_config: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeDocument {
    pub id: String,
    pub from: Endpoint,
    pub to: Endpoint,
    pub frame_type: String,
    pub queue_policy: QueueDocument,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub node_id: String,
    pub port: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueDocument {
    pub capacity: u32,
    pub overflow: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GraphDiagnostic {
    pub code: String,
    pub message: String,
    pub pointer: String,
}

impl fmt::Display for GraphDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}: {}",
            self.code, self.pointer, self.message
        )
    }
}

/// Studio-safe discovery data generated from the same Registry used by compilation.
#[derive(Clone, Debug, Serialize)]
pub struct NodeCatalogEntry {
    pub node_type: String,
    pub language: String,
    pub factory_version: String,
    pub kind: String,
    pub category: String,
    pub capability: String,
    pub summary: String,
    pub documentation: String,
    pub tags: Vec<String>,
    pub ports: Vec<NodeCatalogPort>,
    pub config_schema: serde_json::Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct NodeCatalogPort {
    pub name: String,
    pub direction: String,
    pub frame_type: String,
    pub schema: serde_json::Value,
}

/// Returns the trusted built-ins shipped with this binary.
pub fn builtin_registry() -> NodeRegistry {
    builtins::registry()
}

/// Returns deterministic Studio/CLI discovery metadata from the built-in Registry.
pub fn builtin_node_catalog() -> Vec<NodeCatalogEntry> {
    node_catalog(&builtin_registry())
}

/// Produces serializable discovery metadata without introducing a second descriptor source.
pub fn node_catalog(registry: &NodeRegistry) -> Vec<NodeCatalogEntry> {
    registry
        .entries()
        .map(|registration| {
            let descriptor = registration.descriptor();
            let metadata = builtin_metadata(descriptor.node_type().as_str());
            NodeCatalogEntry {
                node_type: descriptor.node_type().as_str().to_owned(),
                language: registration.language().as_str().to_owned(),
                factory_version: registration.version().as_str().to_owned(),
                kind: match descriptor.kind() {
                    NodeKind::Source => "source",
                    NodeKind::Transform => "transform",
                    NodeKind::Sink => "sink",
                }
                .to_owned(),
                category: metadata.0.to_owned(),
                capability: metadata.1.to_owned(),
                summary: metadata.2.to_owned(),
                documentation: "https://piyotahu.github.io/Voxa/en/providers/builtin/".to_owned(),
                tags: metadata.3.iter().map(|tag| (*tag).to_owned()).collect(),
                ports: descriptor
                    .ports()
                    .iter()
                    .map(|port| NodeCatalogPort {
                        name: port.name().as_str().to_owned(),
                        direction: match port.direction() {
                            PortDirection::Input => "input",
                            PortDirection::Output => "output",
                        }
                        .to_owned(),
                        frame_type: frame_type_name(port.frame_type()).to_owned(),
                        schema: builtin_port_schema(
                            descriptor.node_type().as_str(),
                            port.name().as_str(),
                            port.frame_type(),
                        ),
                    })
                    .collect(),
                config_schema: value_to_json(descriptor.config_schema().value()),
            }
        })
        .collect()
}

fn builtin_metadata(
    node_type: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
) {
    match node_type {
        "builtin.audio_resampler" => (
            "media",
            "audio.resample",
            "Converts PCM audio between sample rates.",
            &["audio", "resample"],
        ),
        "builtin.audio_vad" => (
            "algorithm",
            "speech.vad",
            "Detects speech activity in PCM audio.",
            &["audio", "vad", "speech"],
        ),
        "builtin.voice_turn_context" => (
            "control",
            "conversation.turn_context",
            "Joins transcript and speech events into a turn-aware prompt.",
            &["turn", "context", "voice"],
        ),
        "builtin.interval_tick" => (
            "control",
            "clock.interval",
            "Emits deterministic interval events that drive polling Sources.",
            &["clock", "event"],
        ),
        "builtin.text_source" => (
            "utility",
            "text.source",
            "Emits configured text into a graph.",
            &["text", "source"],
        ),
        "builtin.uppercase" => (
            "utility",
            "text.uppercase",
            "Converts text frames to uppercase.",
            &["text", "transform"],
        ),
        "builtin.text_sink" => (
            "utility",
            "text.collect",
            "Collects text frames in memory.",
            &["text", "sink"],
        ),
        "builtin.stdout_text_sink" => (
            "utility",
            "observability.stdout",
            "Prints text frames to standard output.",
            &["text", "stdout"],
        ),
        value if value.starts_with("builtin.demo.") => (
            "utility",
            "demo.voice",
            "Deterministic architecture-preview Node; not a production Provider.",
            &["demo", "mock"],
        ),
        _ => (
            "utility",
            "runtime.builtin",
            "Voxa runtime built-in Node.",
            &["builtin"],
        ),
    }
}

fn builtin_port_schema(node_type: &str, port: &str, frame_type: FrameType) -> serde_json::Value {
    if frame_type == FrameType::Audio {
        let sample_rate_hz = if node_type == "builtin.audio_resampler" && port == "audio_out" {
            serde_json::Value::String("configured".to_owned())
        } else {
            serde_json::Value::Number(16_000.into())
        };
        return serde_json::json!({
            "encoding": "pcm_s16le",
            "sample_rate_hz": sample_rate_hz,
            "channels": 1,
            "streaming": true
        });
    }
    match frame_type {
        FrameType::Text => serde_json::json!({"encoding": "utf-8"}),
        FrameType::Event => serde_json::json!({"semantics": port}),
        _ => serde_json::json!({}),
    }
}

pub fn parse(input: &str) -> Result<GraphDocument, Vec<GraphDiagnostic>> {
    if input.len() > MAX_DOCUMENT_BYTES {
        return Err(vec![diag(
            "VOXA-GRAPH-SIZE",
            "graph document exceeds 1 MiB",
            "",
        )]);
    }
    let mut document: GraphDocument = serde_json::from_str(input)
        .map_err(|error| vec![diag("VOXA-GRAPH-JSON", &error.to_string(), "")])?;
    migrate_legacy_node_names(&mut document);
    if document.version != "voxa.graph/v1" {
        return Err(vec![diag(
            "VOXA-GRAPH-VERSION",
            "expected voxa.graph/v1",
            "/version",
        )]);
    }
    if document.nodes.len() > 1024 || document.edges.len() > 4096 {
        return Err(vec![diag(
            "VOXA-GRAPH-LIMIT",
            "node or edge limit exceeded",
            "",
        )]);
    }
    Ok(document)
}

fn migrate_legacy_node_names(document: &mut GraphDocument) {
    let mut migrated_agora_sources = Vec::new();
    for node in &mut document.nodes {
        match node.node_type.as_str() {
            "provider.agora.audio_source" => {
                node.node_type = canonical_node_type(&node.node_type).to_owned();
                node.factory_version = "1.1.0".to_owned();
                migrated_agora_sources.push(node.id.clone());
            }
            _ => node.node_type = canonical_node_type(&node.node_type).to_owned(),
        }
    }
    if migrated_agora_sources.is_empty() {
        return;
    }
    document.edges.retain(|edge| {
        !(migrated_agora_sources.contains(&edge.to.node_id) && edge.to.port == "tick_in")
    });
    let referenced = document
        .edges
        .iter()
        .flat_map(|edge| [&edge.from.node_id, &edge.to.node_id])
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    document
        .nodes
        .retain(|node| node.node_type != "builtin.interval_tick" || referenced.contains(&node.id));
}

/// Returns the stable Node type for names written by older Voxa releases.
pub fn canonical_node_type(node_type: &str) -> &str {
    match node_type {
        "provider.agora.audio_source" => "agora.audio_source",
        "provider.agora.audio_sink" => "agora.audio_sink",
        "provider.qwen.audio_realtime" => "qwen.audio_realtime",
        "provider.qwen.asr_realtime" => "qwen.asr_realtime",
        "provider.qwen.llm_stream" => "qwen.llm_stream",
        "provider.qwen.tts_realtime" => "qwen.tts_realtime",
        "builtin.audio_resample" => "builtin.audio_resampler",
        current => current,
    }
}

/// Compiles against the trusted built-ins shipped with Voxa.
pub fn compile(document: &GraphDocument) -> Result<GraphDefinition, Vec<GraphDiagnostic>> {
    compile_with_registry(document, &builtin_registry())
}

/// Compiles Graph v1 using one explicit Registry as the sole Node metadata and Factory source.
pub fn compile_with_registry(
    document: &GraphDocument,
    registry: &NodeRegistry,
) -> Result<GraphDefinition, Vec<GraphDiagnostic>> {
    let graph_id = GraphId::new(document.graph_id.clone())
        .map_err(|error| vec![diag("VOXA-GRAPH-ID", &error.to_string(), "/graph_id")])?;
    let mut builder = GraphBuilder::with_graph_id(graph_id);
    let mut errors = Vec::new();

    for (index, node) in document.nodes.iter().enumerate() {
        if let Err(error) = compile_node(&mut builder, registry, node, index) {
            errors.push(error);
        }
    }
    for (index, edge) in document.edges.iter().enumerate() {
        match edge_descriptor(edge) {
            Ok(descriptor) => {
                if let Err(error) = builder.connect(descriptor) {
                    errors.push(diag(
                        "VOXA-GRAPH-EDGE",
                        &error.to_string(),
                        &format!("/edges/{index}"),
                    ));
                }
            }
            Err(message) => errors.push(diag(
                "VOXA-GRAPH-EDGE",
                &message,
                &format!("/edges/{index}"),
            )),
        }
    }

    if errors.is_empty() {
        builder
            .build()
            .map_err(|error| vec![diag("VOXA-GRAPH-BUILD", &error.to_string(), "")])
    } else {
        Err(errors)
    }
}

fn compile_node(
    builder: &mut GraphBuilder,
    registry: &NodeRegistry,
    node: &NodeDocument,
    index: usize,
) -> Result<(), GraphDiagnostic> {
    let pointer = format!("/nodes/{index}");
    let node_id = NodeId::new(node.id.clone()).map_err(|error| {
        diag(
            "VOXA-GRAPH-NODE-ID",
            &error.to_string(),
            &format!("{pointer}/id"),
        )
    })?;
    let node_type = voxa_core::NodeTypeName::new(node.node_type.clone()).map_err(|error| {
        diag(
            "VOXA-GRAPH-NODE-TYPE",
            &error.to_string(),
            &format!("{pointer}/node_type"),
        )
    })?;
    let language = NodeLanguage::parse(&node.language).ok_or_else(|| {
        diag(
            "VOXA-GRAPH-NODE-LANGUAGE",
            "language must be one of rust, cpp, python, or typescript",
            &format!("{pointer}/language"),
        )
    })?;
    let factory_version =
        NodeFactoryVersion::new(node.factory_version.clone()).map_err(|error| {
            diag(
                "VOXA-GRAPH-FACTORY-VERSION",
                &error.to_string(),
                &format!("{pointer}/factory_version"),
            )
        })?;
    let config = config_map_from_json_object(&node.node_config).map_err(|message| {
        diag(
            "VOXA-GRAPH-CONFIG-VALUE",
            &message,
            &format!("{pointer}/node_config"),
        )
    })?;
    let descriptor = registry
        .descriptor_for(&node_type, language, &factory_version, node_id.clone())
        .map_err(|error| diag("VOXA-GRAPH-REGISTRY", &error.to_string(), &pointer))?;
    registry
        .validate_config(
            &node_type,
            language,
            &factory_version,
            node_id.clone(),
            &config,
        )
        .map_err(|error| {
            diag(
                "VOXA-GRAPH-CONFIG",
                &error.to_string(),
                &format!("{pointer}/node_config"),
            )
        })?;

    builder
        .add_node(descriptor)
        .map_err(|error| diag("VOXA-GRAPH-NODE", &error.to_string(), &pointer))?;
    builder
        .set_config(&node_id, config)
        .and_then(|builder| {
            builder.set_factory(
                &node_id,
                NodeFactorySelection::new(language, factory_version),
            )
        })
        .map_err(|error| diag("VOXA-GRAPH-NODE", &error.to_string(), &pointer))?;
    Ok(())
}

pub fn config_map_from_json_object(
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<ConfigMap, String> {
    let mut converted = Vec::with_capacity(values.len());
    for (key, value) in values {
        let key = ConfigKey::new(key.clone()).map_err(|error| error.to_string())?;
        converted.push((key, value_from_json(value)?));
    }
    ConfigMap::try_from_iter(converted).map_err(|error| error.to_string())
}

pub fn value_from_json(value: &serde_json::Value) -> Result<Value, String> {
    match value {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(value) => Ok(Value::Bool(*value)),
        serde_json::Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                Ok(Value::Integer(integer))
            } else if value.as_u64().is_some() {
                Err("unsigned configuration integer exceeds the i64 protocol range".into())
            } else {
                let float = value
                    .as_f64()
                    .ok_or_else(|| "configuration number is not representable".to_owned())?;
                FiniteF64::new(float)
                    .map(Value::Float)
                    .map_err(|error| error.to_string())
            }
        }
        serde_json::Value::String(value) => Ok(Value::String(value.clone().into_boxed_str())),
        serde_json::Value::Array(values) => values
            .iter()
            .map(value_from_json)
            .collect::<Result<Vec<_>, _>>()
            .map(|values| Value::List(values.into_boxed_slice())),
        serde_json::Value::Object(values) => {
            let mut converted = Vec::with_capacity(values.len());
            for (key, value) in values {
                converted.push((key.as_str(), value_from_json(value)?));
            }
            ValueMap::try_from_iter(converted)
                .map(Value::Map)
                .map_err(|error| error.to_string())
        }
    }
}

pub fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Integer(value) => (*value).into(),
        Value::Float(value) => serde_json::Number::from_f64(value.get())
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.to_string()),
        Value::Bytes(value) => serde_json::Value::Array(
            value
                .as_slice()
                .iter()
                .map(|byte| serde_json::Value::from(*byte))
                .collect(),
        ),
        Value::List(values) => serde_json::Value::Array(values.iter().map(value_to_json).collect()),
        Value::Map(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.to_owned(), value_to_json(value)))
                .collect(),
        ),
    }
}

/// Converts a deterministic Voxa configuration map into a JSON object.
pub fn config_map_to_json(config: &ConfigMap) -> serde_json::Value {
    serde_json::Value::Object(
        config
            .iter()
            .map(|(key, value)| (key.as_str().to_owned(), value_to_json(value)))
            .collect(),
    )
}

fn diag(code: &str, message: &str, pointer: &str) -> GraphDiagnostic {
    GraphDiagnostic {
        code: code.into(),
        message: message.into(),
        pointer: pointer.into(),
    }
}

fn edge_descriptor(edge: &EdgeDocument) -> Result<EdgeDescriptor, String> {
    let frame_type = match edge.frame_type.as_str() {
        "audio" => FrameType::Audio,
        "video" => FrameType::Video,
        "text" => FrameType::Text,
        "byte" => FrameType::Byte,
        "signal" => FrameType::Signal,
        "event" => FrameType::Event,
        _ => return Err("unknown frame_type".into()),
    };
    let overflow = match edge.queue_policy.overflow.as_str() {
        "block" => QueueOverflowPolicy::Block,
        "drop_oldest" => QueueOverflowPolicy::DropOldest,
        "drop_newest" => QueueOverflowPolicy::DropNewest,
        "abort" => QueueOverflowPolicy::Abort,
        _ => return Err("unknown queue overflow".into()),
    };
    let capacity = NonZeroUsize::new(edge.queue_policy.capacity as usize)
        .ok_or("queue capacity must be nonzero")?;
    Ok(EdgeDescriptor::new(
        EdgeId::new(edge.id.clone()).map_err(|error| error.to_string())?,
        NodeId::new(edge.from.node_id.clone()).map_err(|error| error.to_string())?,
        voxa_core::PortName::new(edge.from.port.clone()).map_err(|error| error.to_string())?,
        NodeId::new(edge.to.node_id.clone()).map_err(|error| error.to_string())?,
        voxa_core::PortName::new(edge.to.port.clone()).map_err(|error| error.to_string())?,
        frame_type,
        QueuePolicy::new(capacity, overflow),
        ValidationPolicy::TypeGateOnly,
        TransformPolicy::Identity,
        EnabledCondition::Always,
        VisibilityDescriptor::default(),
    ))
}

fn frame_type_name(frame_type: FrameType) -> &'static str {
    match frame_type {
        FrameType::Audio => "audio",
        FrameType::Video => "video",
        FrameType::Text => "text",
        FrameType::Byte => "byte",
        FrameType::Signal => "signal",
        FrameType::Event => "event",
    }
}
