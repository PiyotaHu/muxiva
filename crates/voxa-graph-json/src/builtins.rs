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
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, ErrorCategory, EventData,
    Extensions, Frame, FrameBuffer, FrameDerivation, FrameHeader, FrameId, FramePayload, FrameType,
    Lineage, Metadata, NamespacedName, NodeId, PcmSampleFormat, SchemaVersion, SequenceId,
    StreamId, TextData, Timestamp, TraceId, TransformOrigin, Value, ValueMap, VoxaError,
};

pub const BUILTIN_FACTORY_VERSION: &str = "1.0.0";
pub const TEXT_SOURCE: &str = "builtin.text_source";
pub const UPPERCASE: &str = "builtin.uppercase";
pub const TEXT_SINK: &str = "builtin.text_sink";
pub const STDOUT_TEXT_SINK: &str = "builtin.stdout_text_sink";
pub const DEMO_MICROPHONE: &str = "builtin.demo.microphone";
pub const DEMO_STREAMING_ASR: &str = "builtin.demo.streaming_asr";
pub const DEMO_VOICE_ACTIVITY: &str = "builtin.demo.voice_activity";
pub const DEMO_CONTEXT_FUSION: &str = "builtin.demo.context_fusion";
pub const DEMO_REASONING_LLM: &str = "builtin.demo.reasoning_llm";
pub const DEMO_NEURAL_TTS: &str = "builtin.demo.neural_tts";
pub const DEMO_SPEAKER: &str = "builtin.demo.speaker";
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
    register(
        &mut registry,
        descriptor(
            STDOUT_TEXT_SINK,
            NodeKind::Sink,
            &[(TEXT_INPUT, PortDirection::Input)],
            empty_schema(),
        ),
        Arc::new(StdoutTextSinkFactory),
    );
    register_demo_nodes(&mut registry);
    registry
}

fn register_demo_nodes(registry: &mut NodeRegistry) {
    for (node_type, kind, ports, factory) in [
        (
            DEMO_MICROPHONE,
            NodeKind::Source,
            vec![("audio_out", PortDirection::Output, FrameType::Audio)],
            Arc::new(DemoFactory(DemoNodeKind::Microphone)) as Arc<dyn NodeFactory>,
        ),
        (
            DEMO_STREAMING_ASR,
            NodeKind::Transform,
            vec![
                ("audio_in", PortDirection::Input, FrameType::Audio),
                ("transcript_out", PortDirection::Output, FrameType::Text),
            ],
            Arc::new(DemoFactory(DemoNodeKind::Asr)),
        ),
        (
            DEMO_VOICE_ACTIVITY,
            NodeKind::Transform,
            vec![
                ("audio_in", PortDirection::Input, FrameType::Audio),
                ("speech_out", PortDirection::Output, FrameType::Event),
            ],
            Arc::new(DemoFactory(DemoNodeKind::Vad)),
        ),
        (
            DEMO_CONTEXT_FUSION,
            NodeKind::Transform,
            vec![
                ("transcript_in", PortDirection::Input, FrameType::Text),
                ("speech_in", PortDirection::Input, FrameType::Event),
                ("context_out", PortDirection::Output, FrameType::Text),
            ],
            Arc::new(DemoFactory(DemoNodeKind::Fusion)),
        ),
        (
            DEMO_REASONING_LLM,
            NodeKind::Transform,
            vec![
                ("context_in", PortDirection::Input, FrameType::Text),
                ("response_out", PortDirection::Output, FrameType::Text),
            ],
            Arc::new(DemoFactory(DemoNodeKind::Llm)),
        ),
        (
            DEMO_NEURAL_TTS,
            NodeKind::Transform,
            vec![
                ("text_in", PortDirection::Input, FrameType::Text),
                ("audio_out", PortDirection::Output, FrameType::Audio),
            ],
            Arc::new(DemoFactory(DemoNodeKind::Tts)),
        ),
        (
            DEMO_SPEAKER,
            NodeKind::Sink,
            vec![("audio_in", PortDirection::Input, FrameType::Audio)],
            Arc::new(DemoFactory(DemoNodeKind::Speaker)),
        ),
    ] {
        register(
            registry,
            typed_descriptor(node_type, kind, &ports, empty_schema()),
            factory,
        );
    }
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
    let typed_ports = ports
        .iter()
        .map(|(name, direction)| (*name, *direction, FrameType::Text))
        .collect::<Vec<_>>();
    typed_descriptor(node_type, kind, &typed_ports, config_schema)
}

fn typed_descriptor(
    node_type: &str,
    kind: NodeKind,
    ports: &[(&str, PortDirection, FrameType)],
    config_schema: ConfigSchema,
) -> NodeDescriptor {
    let template_id = NodeId::new(format!("template-{node_type}")).expect("valid template ID");
    NodeDescriptor::new(
        template_id.clone(),
        NodeTypeName::new(node_type).expect("valid built-in node type"),
        kind,
        ports
            .iter()
            .map(|(name, direction, frame_type)| {
                PortDescriptor::new(
                    template_id.clone(),
                    PortName::new(*name).expect("valid built-in port"),
                    *direction,
                    *frame_type,
                )
            })
            .collect::<Vec<_>>(),
        config_schema,
        LifecycleCapabilities::default(),
    )
}

#[derive(Clone, Copy)]
enum DemoNodeKind {
    Microphone,
    Asr,
    Vad,
    Fusion,
    Llm,
    Tts,
    Speaker,
}

struct DemoFactory(DemoNodeKind);

impl NodeFactory for DemoFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        validate_empty_config(config, "demo node")
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(match self.0 {
            DemoNodeKind::Microphone => Box::new(DemoMicrophone) as Box<dyn Node>,
            DemoNodeKind::Asr => Box::new(DemoAsr),
            DemoNodeKind::Vad => Box::new(DemoVad),
            DemoNodeKind::Fusion => Box::new(DemoContextFusion::default()),
            DemoNodeKind::Llm => Box::new(DemoLlm),
            DemoNodeKind::Tts => Box::new(DemoTts),
            DemoNodeKind::Speaker => Box::new(DemoSpeaker),
        })
    }
}

struct DemoMicrophone;
impl Node for DemoMicrophone {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        if input.is_some() {
            return Err(node_error(
                "VOXA-DEMO-SOURCE-INPUT",
                "demo microphone received input",
            ));
        }
        println!("[VOXA][FRAME][{}] audio=pcm_s16le rate_hz=16000 channels=1 duration_ms=20 provider=mock", context.node_id());
        context.emit(
            PortName::new("audio_out").unwrap(),
            demo_audio_frame("microphone")?,
        )?;
        Ok(())
    }
}

struct DemoAsr;
impl Node for DemoAsr {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "streaming ASR requires audio")?;
        let transcript = "How can Voxa power a real-time voice agent?";
        println!(
            "[VOXA][NODE][{}] transcript=\"{transcript}\" provider=mock",
            context.node_id()
        );
        context.emit(
            PortName::new("transcript_out").unwrap(),
            derive_payload(
                &input,
                context.node_id(),
                "asr",
                FramePayload::Text(TextData::new(transcript)),
            )?,
        )?;
        Ok(())
    }
}

struct DemoVad;
impl Node for DemoVad {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = required_type(
            input,
            FrameType::Audio,
            "voice activity detector requires audio",
        )?;
        println!(
            "[VOXA][NODE][{}] speech_detected=true confidence=0.98 provider=mock",
            context.node_id()
        );
        let event = EventData::new(
            NamespacedName::new("voxa.demo.speech.detected")?,
            SchemaVersion::new(1)?,
            context.node_id().clone(),
            Value::Bool(true),
        );
        context.emit(
            PortName::new("speech_out").unwrap(),
            derive_payload(&input, context.node_id(), "vad", FramePayload::Event(event))?,
        )?;
        Ok(())
    }
}

#[derive(Default)]
struct DemoContextFusion {
    transcript: Option<Box<str>>,
    speech: bool,
    emitted: bool,
}
impl Node for DemoContextFusion {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error("VOXA-DEMO-INPUT-MISSING", "context fusion requires input")
        })?;
        match context.input_port().map(PortName::as_str) {
            Some("transcript_in") => {
                self.transcript = Some(
                    input
                        .as_text()
                        .ok_or_else(|| {
                            node_error("VOXA-DEMO-INPUT-TYPE", "transcript input must be text")
                        })?
                        .data()
                        .as_str()
                        .into(),
                )
            }
            Some("speech_in") => {
                input.ensure_type(FrameType::Event)?;
                self.speech = true;
            }
            _ => {
                return Err(node_error(
                    "VOXA-DEMO-INPUT-PORT",
                    "context fusion received an unknown port",
                ))
            }
        }
        if self.speech && self.transcript.is_some() && !self.emitted {
            self.emitted = true;
            let transcript = self.transcript.as_deref().unwrap();
            println!(
                "[VOXA][JOIN][{}] inputs=transcript+speech_event status=ready",
                context.node_id()
            );
            let prompt = format!("speech=true; user={transcript}");
            context.emit(
                PortName::new("context_out").unwrap(),
                derive_payload(
                    &input,
                    context.node_id(),
                    "context-fusion",
                    FramePayload::Text(TextData::new(prompt)),
                )?,
            )?;
        }
        Ok(())
    }
}

struct DemoLlm;
impl Node for DemoLlm {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = required_type(input, FrameType::Text, "reasoning LLM requires context")?;
        let response =
            "Voxa runs audio, ASR, VAD, reasoning, and TTS as one typed, concurrent graph.";
        println!(
            "[VOXA][NODE][{}] tokens_in=14 tokens_out=18 provider=mock",
            context.node_id()
        );
        context.emit(
            PortName::new("response_out").unwrap(),
            derive_payload(
                &input,
                context.node_id(),
                "reasoning-llm",
                FramePayload::Text(TextData::new(response)),
            )?,
        )?;
        Ok(())
    }
}

struct DemoTts;
impl Node for DemoTts {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = required_type(input, FrameType::Text, "neural TTS requires text")?;
        println!(
            "[VOXA][NODE][{}] voice=alloy audio=pcm_s16le duration_ms=20 provider=mock",
            context.node_id()
        );
        let audio = AudioData::new(
            FrameBuffer::from_vec(vec![0; 640]),
            16_000,
            1,
            PcmSampleFormat::I16Le,
            AudioLayout::Interleaved,
            320,
        )?;
        context.emit(
            PortName::new("audio_out").unwrap(),
            derive_payload(
                &input,
                context.node_id(),
                "neural-tts",
                FramePayload::Audio(audio),
            )?,
        )?;
        Ok(())
    }
}

struct DemoSpeaker;
impl Node for DemoSpeaker {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "speaker requires audio")?;
        let duration_ms = input.as_audio().unwrap().data().duration_ns() / 1_000_000;
        println!(
            "[VOXA][RESULT][{}] played_audio_ms={duration_ms} provider=mock",
            context.node_id()
        );
        Ok(())
    }
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

struct StdoutTextSinkFactory;

impl NodeFactory for StdoutTextSinkFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        validate_empty_config(config, "stdout text sink")
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(StdoutTextSink))
    }
}

/// An explicitly side-effecting development sink used by CLI demos.
struct StdoutTextSink;

impl Node for StdoutTextSink {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "VOXA-BUILTIN-INPUT-MISSING",
                "stdout text sink requires a text input",
            )
        })?;
        let text = input.as_text().ok_or_else(|| {
            node_error(
                "VOXA-BUILTIN-INPUT-TYPE",
                "stdout text sink requires a text frame",
            )
        })?;
        println!(
            "[VOXA][RESULT][{}] {}",
            context.node_id(),
            text.data().as_str()
        );
        Ok(())
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

fn demo_audio_frame(origin: &str) -> voxa_types::Result<Frame> {
    let audio = AudioData::new(
        FrameBuffer::from_vec(vec![0; 640]),
        16_000,
        1,
        PcmSampleFormat::I16Le,
        AudioLayout::Interleaved,
        320,
    )?;
    demo_frame(origin, FramePayload::Audio(audio))
}

fn demo_frame(origin: &str, payload: FramePayload) -> voxa_types::Result<Frame> {
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    let frame_type = payload.frame_type();
    Frame::new(
        FrameHeader::new(
            FrameId::new(format!("demo-{origin}-{serial}")).expect("bounded demo frame ID"),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("voxa.demo.media").expect("valid demo clock"),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(0),
            StreamId::new(format!("demo-stream-{serial}")).expect("bounded demo stream ID"),
            TraceId::new(format!("demo-trace-{serial}")).expect("bounded demo trace ID"),
            frame_type,
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )?,
        payload,
    )
}

fn required_type(
    input: Option<Frame>,
    frame_type: FrameType,
    message: &'static str,
) -> voxa_types::Result<Frame> {
    let input = input.ok_or_else(|| node_error("VOXA-DEMO-INPUT-MISSING", message))?;
    input.ensure_type(frame_type)?;
    Ok(input)
}

fn derive_payload(
    input: &Frame,
    node_id: &NodeId,
    reason: &'static str,
    payload: FramePayload,
) -> voxa_types::Result<Frame> {
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    input.derive(
        FrameDerivation::new(
            FrameId::new(format!("demo-{reason}-{serial}")).expect("bounded demo frame ID"),
            input.header().timestamp(),
            input.header().sequence_id(),
            TransformOrigin::new(Some(node_id.clone()), None)?,
            reason,
        )?
        .with_payload(payload),
    )
}
