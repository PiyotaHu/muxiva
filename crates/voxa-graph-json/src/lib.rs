//! Strict Graph v1 configuration parser and compiler.
use serde::{Deserialize, Serialize};
use std::{fmt, num::NonZeroUsize};
use voxa_core::{
    ConfigSchema, EdgeDescriptor, EnabledCondition, GraphBuilder, GraphDefinition,
    LifecycleCapabilities, NodeDescriptor, NodeKind, PortDescriptor, PortDirection, PortName,
    QueueOverflowPolicy, QueuePolicy, TransformPolicy, ValidationPolicy, VisibilityDescriptor,
};
use voxa_types::{EdgeId, FrameType, GraphId, NodeId};

pub const GRAPH_V1_SCHEMA: &str = r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","title":"Voxa Graph v1","type":"object","required":["version","graph_id","nodes","edges"],"properties":{"version":{"const":"voxa.graph/v1"},"graph_id":{"type":"string","maxLength":255},"nodes":{"type":"array","maxItems":1024},"edges":{"type":"array","maxItems":4096}},"additionalProperties":false}"#;
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
    #[serde(default = "rust_language")]
    pub language: String,
    #[serde(default)]
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
fn rust_language() -> String {
    "rust".into()
}
#[derive(Clone, Debug, Serialize)]
pub struct GraphDiagnostic {
    pub code: String,
    pub message: String,
    pub pointer: String,
}
impl fmt::Display for GraphDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.pointer, self.message)
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
    let doc: GraphDocument = serde_json::from_str(input)
        .map_err(|e| vec![diag("VOXA-GRAPH-JSON", &e.to_string(), "")])?;
    if doc.version != "voxa.graph/v1" {
        return Err(vec![diag(
            "VOXA-GRAPH-VERSION",
            "expected voxa.graph/v1",
            "/version",
        )]);
    }
    if doc.nodes.len() > 1024 || doc.edges.len() > 4096 {
        return Err(vec![diag(
            "VOXA-GRAPH-LIMIT",
            "node or edge limit exceeded",
            "",
        )]);
    }
    Ok(doc)
}
pub fn compile(doc: &GraphDocument) -> Result<GraphDefinition, Vec<GraphDiagnostic>> {
    let id = GraphId::new(doc.graph_id.clone())
        .map_err(|e| vec![diag("VOXA-GRAPH-ID", &e.to_string(), "/graph_id")])?;
    let mut builder = GraphBuilder::with_graph_id(id);
    let mut errors = Vec::new();
    for (i, node) in doc.nodes.iter().enumerate() {
        match descriptor(node) {
            Ok(d) => {
                if let Err(e) = builder.add_node(d) {
                    errors.push(diag(
                        "VOXA-GRAPH-NODE",
                        &e.to_string(),
                        &format!("/nodes/{i}"),
                    ))
                }
            }
            Err(e) => errors.push(diag("VOXA-GRAPH-NODE", &e, &format!("/nodes/{i}"))),
        }
    }
    for (i, edge) in doc.edges.iter().enumerate() {
        match edge_descriptor(edge) {
            Ok(e) => {
                if let Err(e) = builder.connect(e) {
                    errors.push(diag(
                        "VOXA-GRAPH-EDGE",
                        &e.to_string(),
                        &format!("/edges/{i}"),
                    ))
                }
            }
            Err(e) => errors.push(diag("VOXA-GRAPH-EDGE", &e, &format!("/edges/{i}"))),
        }
    }
    if errors.is_empty() {
        builder
            .build()
            .map_err(|e| vec![diag("VOXA-GRAPH-BUILD", &e.to_string(), "")])
    } else {
        Err(errors)
    }
}
fn diag(code: &str, message: &str, pointer: &str) -> GraphDiagnostic {
    GraphDiagnostic {
        code: code.into(),
        message: message.into(),
        pointer: pointer.into(),
    }
}
fn descriptor(n: &NodeDocument) -> Result<NodeDescriptor, String> {
    if n.language != "rust" {
        return Err("only compiled-in Rust registrations are runnable in this build".into());
    }
    let id = NodeId::new(n.id.clone()).map_err(|e| e.to_string())?;
    let (kind, ports) = match n.node_type.as_str() {
        "builtin.text_source" => (NodeKind::Source, vec![("text_out", PortDirection::Output)]),
        "builtin.uppercase" => (
            NodeKind::Transform,
            vec![
                ("text_in", PortDirection::Input),
                ("text_out", PortDirection::Output),
            ],
        ),
        "builtin.text_sink" => (NodeKind::Sink, vec![("text_in", PortDirection::Input)]),
        _ => return Err(format!("unknown trusted node type `{}`", n.node_type)),
    };
    Ok(NodeDescriptor::new(
        id,
        voxa_core::NodeTypeName::new(n.node_type.clone()).map_err(|e| e.to_string())?,
        kind,
        ports
            .into_iter()
            .map(|(name, d)| {
                PortDescriptor::new(
                    NodeId::new(n.id.clone()).unwrap(),
                    PortName::new(name).unwrap(),
                    d,
                    FrameType::Text,
                )
            })
            .collect::<Vec<_>>(),
        ConfigSchema::empty(),
        LifecycleCapabilities::default(),
    ))
}
fn edge_descriptor(e: &EdgeDocument) -> Result<EdgeDescriptor, String> {
    let ft = if e.frame_type == "text" {
        FrameType::Text
    } else {
        return Err("only text frame_type is registered".into());
    };
    let overflow = match e.queue_policy.overflow.as_str() {
        "block" => QueueOverflowPolicy::Block,
        "drop_oldest" => QueueOverflowPolicy::DropOldest,
        "drop_newest" => QueueOverflowPolicy::DropNewest,
        "abort" => QueueOverflowPolicy::Abort,
        _ => return Err("unknown queue overflow".into()),
    };
    let cap = NonZeroUsize::new(e.queue_policy.capacity as usize)
        .ok_or("queue capacity must be nonzero")?;
    Ok(EdgeDescriptor::new(
        EdgeId::new(e.id.clone()).map_err(|x| x.to_string())?,
        NodeId::new(e.from.node_id.clone()).map_err(|x| x.to_string())?,
        PortName::new(e.from.port.clone()).map_err(|x| x.to_string())?,
        NodeId::new(e.to.node_id.clone()).map_err(|x| x.to_string())?,
        PortName::new(e.to.port.clone()).map_err(|x| x.to_string())?,
        ft,
        QueuePolicy::new(cap, overflow),
        ValidationPolicy::TypeGateOnly,
        TransformPolicy::Identity,
        EnabledCondition::Always,
        VisibilityDescriptor::default(),
    ))
}
