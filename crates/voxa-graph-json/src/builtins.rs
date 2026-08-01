use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use voxa_core::{
    ConfigMap, ConfigSchema, LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeFactory,
    NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration, NodeRegistry,
    NodeTypeName, PortDescriptor, PortDirection, PortName,
};
use voxa_types::{
    ClockDomain, ClockDomainId, ClockKind, ErrorCategory, Extensions, Frame, FrameDerivation,
    FrameHeader, FrameId, FramePayload, FrameType, Lineage, Metadata, NodeId, SequenceId, StreamId,
    TextData, Timestamp, TraceId, TransformOrigin, Value, ValueMap, VoxaError,
};

pub const BUILTIN_FACTORY_VERSION: &str = "1.0.0";
pub const TEXT_SOURCE: &str = "builtin.text_source";
pub const UPPERCASE: &str = "builtin.uppercase";
pub const TEXT_SINK: &str = "builtin.text_sink";
const TEXT_INPUT: &str = "text_in";
const TEXT_OUTPUT: &str = "text_out";
const MAX_TEXT_BYTES: usize = 256 * 1024;
static NEXT_FRAME: AtomicU64 = AtomicU64::new(1);

pub fn registry() -> NodeRegistry {
    let mut registry = NodeRegistry::default();
    register(
        &mut registry,
        descriptor(
            TEXT_SOURCE,
            NodeKind::Source,
            &[(TEXT_OUTPUT, PortDirection::Output)],
            text_source_schema(),
        ),
        Arc::new(TextSourceFactory),
    );
    register(
        &mut registry,
        descriptor(
            UPPERCASE,
            NodeKind::Transform,
            &[
                (TEXT_INPUT, PortDirection::Input),
                (TEXT_OUTPUT, PortDirection::Output),
            ],
            empty_schema(),
        ),
        Arc::new(UppercaseFactory),
    );
    register(
        &mut registry,
        descriptor(
            TEXT_SINK,
            NodeKind::Sink,
            &[(TEXT_INPUT, PortDirection::Input)],
            empty_schema(),
        ),
        Arc::new(TextSinkFactory),
    );
    registry
}

fn register(
    registry: &mut NodeRegistry,
    descriptor: NodeDescriptor,
    factory: Arc<dyn NodeFactory>,
) {
    registry
        .register(NodeRegistration::new(
            NodeLanguage::Rust,
            descriptor,
            NodeFactoryVersion::new(BUILTIN_FACTORY_VERSION).expect("valid built-in version"),
            factory,
        ))
        .expect("built-in registrations are unique and valid");
}

fn descriptor(
    node_type: &str,
    kind: NodeKind,
    ports: &[(&str, PortDirection)],
    config_schema: ConfigSchema,
) -> NodeDescriptor {
    let template_id = NodeId::new(format!("template-{node_type}")).expect("valid template ID");
    NodeDescriptor::new(
        template_id.clone(),
        NodeTypeName::new(node_type).expect("valid built-in node type"),
        kind,
        ports
            .iter()
            .map(|(name, direction)| {
                PortDescriptor::new(
                    template_id.clone(),
                    PortName::new(*name).expect("valid built-in port"),
                    *direction,
                    FrameType::Text,
                )
            })
            .collect::<Vec<_>>(),
        config_schema,
        LifecycleCapabilities::default(),
    )
}

fn text_source_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([(
                "text",
                map([
                    ("type", Value::String("string".into())),
                    ("maxLength", Value::Integer(MAX_TEXT_BYTES as i64)),
                    ("default", Value::String("hello".into())),
                ]),
            )]),
        ),
        (
            "required",
            Value::List(vec![Value::String("text".into())].into_boxed_slice()),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

fn empty_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        ("properties", map([])),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Map(ValueMap::try_from_iter(entries).expect("valid built-in schema"))
}

struct TextSourceFactory;

impl NodeFactory for TextSourceFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if config.len() != 1 {
            return Err(config_error("text source accepts exactly the `text` field"));
        }
        match config.get("text") {
            Some(Value::String(text)) if text.len() <= MAX_TEXT_BYTES => Ok(()),
            Some(Value::String(_)) => Err(config_error("`text` exceeds 256 KiB")),
            Some(_) => Err(config_error("`text` must be a string")),
            None => Err(config_error("required field `text` is missing")),
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let Some(Value::String(text)) = config.get("text") else {
            return Err(config_error("validated `text` field is unavailable"));
        };
        Ok(Box::new(TextSource { text: text.clone() }))
    }
}

struct TextSource {
    text: Box<str>,
}

impl Node for TextSource {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        if input.is_some() {
            return Err(node_error(
                "VOXA-BUILTIN-SOURCE-INPUT",
                "source received input",
            ));
        }
        context.emit(
            PortName::new(TEXT_OUTPUT).expect("valid built-in port"),
            source_frame(&self.text)?,
        )?;
        Ok(())
    }
}

struct UppercaseFactory;

impl NodeFactory for UppercaseFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        validate_empty_config(config, "uppercase")
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(Uppercase))
    }
}

struct Uppercase;

impl Node for Uppercase {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "VOXA-BUILTIN-INPUT-MISSING",
                "uppercase transform requires a text input",
            )
        })?;
        let text = input.as_text().ok_or_else(|| {
            node_error(
                "VOXA-BUILTIN-INPUT-TYPE",
                "uppercase transform requires a text frame",
            )
        })?;
        let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
        let derived = input.derive(
            FrameDerivation::new(
                FrameId::new(format!("builtin-uppercase-{serial}"))
                    .expect("bounded built-in frame ID"),
                input.header().timestamp(),
                input.header().sequence_id(),
                TransformOrigin::new(Some(context.node_id().clone()), None)?,
                "builtin_uppercase",
            )?
            .with_payload(FramePayload::Text(TextData::new(
                text.data().as_str().to_uppercase(),
            ))),
        )?;
        context.emit(
            PortName::new(TEXT_OUTPUT).expect("valid built-in port"),
            derived,
        )?;
        Ok(())
    }
}

struct TextSinkFactory;

impl NodeFactory for TextSinkFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        validate_empty_config(config, "text sink")
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(TextSink))
    }
}

struct TextSink;

impl Node for TextSink {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "VOXA-BUILTIN-INPUT-MISSING",
                "text sink requires a text input",
            )
        })?;
        input.ensure_type(FrameType::Text)
    }
}

fn validate_empty_config(config: &ConfigMap, name: &str) -> Result<(), NodeFactoryError> {
    if config.is_empty() {
        Ok(())
    } else {
        Err(config_error(format!(
            "{name} does not accept configuration"
        )))
    }
}

fn config_error(message: impl Into<Box<str>>) -> NodeFactoryError {
    NodeFactoryError::new("VOXA-BUILTIN-CONFIG", message)
}

fn node_error(code: &'static str, message: &'static str) -> VoxaError {
    VoxaError::new(ErrorCategory::Internal, code, message)
}

fn source_frame(text: &str) -> voxa_types::Result<Frame> {
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    Frame::new(
        FrameHeader::new(
            FrameId::new(format!("builtin-source-{serial}")).expect("bounded built-in frame ID"),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("voxa.builtin.text").expect("valid built-in clock"),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(0),
            StreamId::new(format!("builtin-stream-{serial}")).expect("bounded built-in stream ID"),
            TraceId::new(format!("builtin-trace-{serial}")).expect("bounded built-in trace ID"),
            FrameType::Text,
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )?,
        FramePayload::Text(TextData::new(text)),
    )
}
