use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use muxiva_core::{
    ConfigMap, ConfigSchema, LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeFactory,
    NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration, NodeRegistry,
    NodeTypeName, PortDescriptor, PortDirection, PortName,
};
use muxiva_types::{
    AudioData, AudioLayout, ByteData, ClockDomain, ClockDomainId, ClockKind, ErrorCategory,
    EventData, Extensions, Frame, FrameBuffer, FrameDerivation, FrameHeader, FrameId, FramePayload,
    FrameType, Lineage, MediaType, Metadata, MuxivaError, NamespacedName, NodeId, PcmSampleFormat,
    SchemaVersion, SequenceId, SignalData, StreamId, TextData, Timestamp, TraceId, TransformOrigin,
    Value, ValueMap,
};

pub const BUILTIN_FACTORY_VERSION: &str = "1.0.0";
pub const TEXT_SOURCE: &str = "builtin.text_source";
pub const UPPERCASE: &str = "builtin.uppercase";
pub const TEXT_SINK: &str = "builtin.text_sink";
pub const STDOUT_TEXT_SINK: &str = "builtin.stdout_text_sink";
pub const AUDIO_RESAMPLER: &str = "builtin.audio_resampler";
pub const INTERVAL_TICK: &str = "builtin.interval_tick";
pub const AUDIO_VAD: &str = "builtin.audio_vad";
pub const VOICE_TURN_CONTEXT: &str = "builtin.voice_turn_context";
pub const CLIENT_EVENT_ENCODER: &str = "builtin.client_event_encoder";
pub const TEXT_CANCELLATION_GATE: &str = "builtin.text_cancellation_gate";
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
    register_audio_nodes(&mut registry);
    registry
}

fn register_audio_nodes(registry: &mut NodeRegistry) {
    register(
        registry,
        typed_descriptor(
            CLIENT_EVENT_ENCODER,
            NodeKind::Transform,
            &[
                ("event_in", PortDirection::Input, FrameType::Event),
                ("signal_in", PortDirection::Input, FrameType::Signal),
                ("message_out", PortDirection::Output, FrameType::Byte),
            ],
            empty_schema(),
        ),
        Arc::new(ClientEventEncoderFactory),
    );
    register(
        registry,
        typed_descriptor(
            TEXT_CANCELLATION_GATE,
            NodeKind::Transform,
            &[
                ("text_in", PortDirection::Input, FrameType::Text),
                ("signal_in", PortDirection::Input, FrameType::Signal),
                ("text_out", PortDirection::Output, FrameType::Text),
            ],
            empty_schema(),
        ),
        Arc::new(TextCancellationGateFactory),
    );
    register(
        registry,
        typed_descriptor(
            INTERVAL_TICK,
            NodeKind::Source,
            &[("tick_out", PortDirection::Output, FrameType::Event)],
            interval_tick_schema(),
        ),
        Arc::new(IntervalTickFactory),
    );
    register(
        registry,
        typed_descriptor(
            AUDIO_RESAMPLER,
            NodeKind::Transform,
            &[
                ("audio_in", PortDirection::Input, FrameType::Audio),
                ("audio_out", PortDirection::Output, FrameType::Audio),
            ],
            audio_resample_schema(),
        ),
        Arc::new(AudioResampleFactory),
    );
    register(
        registry,
        typed_descriptor(
            AUDIO_VAD,
            NodeKind::Transform,
            &[
                ("audio_in", PortDirection::Input, FrameType::Audio),
                ("speech_out", PortDirection::Output, FrameType::Event),
                ("signal_out", PortDirection::Output, FrameType::Signal),
            ],
            audio_vad_schema(),
        ),
        Arc::new(AudioVadFactory),
    );
    register(
        registry,
        typed_descriptor(
            VOICE_TURN_CONTEXT,
            NodeKind::Transform,
            &[
                ("transcript_in", PortDirection::Input, FrameType::Text),
                ("speech_in", PortDirection::Input, FrameType::Event),
                ("context_out", PortDirection::Output, FrameType::Text),
            ],
            empty_schema(),
        ),
        Arc::new(VoiceTurnContextFactory),
    );
}

struct ClientEventEncoderFactory;

impl NodeFactory for ClientEventEncoderFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if config.is_empty() {
            Ok(())
        } else {
            Err(config_error(
                "client event encoder does not accept configuration",
            ))
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(ClientEventEncoder {
            cancelled_through_sequence: 0,
        }))
    }
}

struct ClientEventEncoder {
    cancelled_through_sequence: u64,
}

impl Node for ClientEventEncoder {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "MUXIVA-CLIENT-EVENT-INPUT",
                "client event encoder requires an Event Frame",
            )
        })?;
        let event = input.as_event().ok_or_else(|| {
            node_error(
                "MUXIVA-CLIENT-EVENT-TYPE",
                "client event encoder accepts Event Frames only",
            )
        })?;
        let data = event.data();
        if data.topic().as_str().starts_with("muxiva.voice.response.")
            && input.header().sequence_id().get() <= self.cancelled_through_sequence
        {
            return Ok(());
        }
        let mut payload = crate::value_to_json(data.payload());
        if let Some(encoded) = payload.as_str() {
            if let Ok(decoded) = serde_json::from_str(encoded) {
                payload = decoded;
            }
        }
        let envelope = serde_json::json!({
            "version": "muxiva.client-event/v1",
            "type": data.topic().as_str(),
            "source": data.source().as_str(),
            "stream_id": input.header().stream_id().as_str(),
            "trace_id": input.header().trace_id().as_str(),
            "sequence": input.header().sequence_id().get(),
            "timestamp_ns": input.header().timestamp().as_nanos(),
            "payload": payload,
        });
        let encoded = serde_json::to_vec(&envelope).map_err(|_| {
            node_error(
                "MUXIVA-CLIENT-EVENT-ENCODE",
                "client event JSON serialization failed",
            )
        })?;
        emit_client_message(&input, context, &encoded)
    }

    fn on_signal(
        &mut self,
        signal: muxiva_types::SignalFrame,
        _context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.cancelled_through_sequence = self
            .cancelled_through_sequence
            .max(signal.header().sequence_id().get());
        Ok(())
    }
}

struct TextCancellationGateFactory;

impl NodeFactory for TextCancellationGateFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if config.is_empty() {
            Ok(())
        } else {
            Err(config_error(
                "text cancellation gate does not accept configuration",
            ))
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(TextCancellationGate {
            cancelled_through_sequence: 0,
        }))
    }
}

struct TextCancellationGate {
    cancelled_through_sequence: u64,
}

impl Node for TextCancellationGate {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = required_type(
            input,
            FrameType::Text,
            "text cancellation gate requires text",
        )?;
        if input.header().sequence_id().get() > self.cancelled_through_sequence {
            context.emit(PortName::new("text_out").unwrap(), input)?;
        }
        Ok(())
    }

    fn on_signal(
        &mut self,
        signal: muxiva_types::SignalFrame,
        _context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.cancelled_through_sequence = self
            .cancelled_through_sequence
            .max(signal.header().sequence_id().get());
        Ok(())
    }
}

fn emit_client_message(
    parent: &Frame,
    context: &mut NodeContext,
    encoded: &[u8],
) -> muxiva_types::Result<()> {
    for payload in client_message_payloads(parent.header().frame_id().as_str(), encoded)? {
        let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
        let output = parent.derive(
            FrameDerivation::new(
                FrameId::new(format!("client-event-{serial}"))
                    .expect("bounded client event frame ID"),
                parent.header().timestamp(),
                parent.header().sequence_id(),
                TransformOrigin::new(Some(context.node_id().clone()), None)?,
                "client_event_encode",
            )?
            .with_payload(FramePayload::Byte(ByteData::new(
                FrameBuffer::from_vec(payload),
                Some(
                    MediaType::new("application/vnd.muxiva.client-event+json")
                        .expect("valid client event media type"),
                ),
            ))),
        )?;
        context.emit(PortName::new("message_out").unwrap(), output)?;
    }
    Ok(())
}

fn client_message_payloads(message_id: &str, encoded: &[u8]) -> muxiva_types::Result<Vec<Vec<u8>>> {
    const TRANSPORT_MESSAGE_LIMIT: usize = 1_024;
    const FRAGMENT_BYTES: usize = 512;
    const MAXIMUM_FRAGMENTS: usize = 64;
    let fragments = encoded.len().div_ceil(FRAGMENT_BYTES).max(1);
    if fragments > MAXIMUM_FRAGMENTS {
        return Err(node_error(
            "MUXIVA-CLIENT-EVENT-LIMIT",
            "encoded client event exceeds the 32 KiB transport limit",
        ));
    }
    let mut messages = Vec::with_capacity(fragments);
    for (index, bytes) in encoded.chunks(FRAGMENT_BYTES).enumerate() {
        let payload = if fragments == 1 && encoded.len() <= TRANSPORT_MESSAGE_LIMIT {
            encoded.to_vec()
        } else {
            serde_json::to_vec(&serde_json::json!({
                "version": "muxiva.transport-fragment/v1",
                "message_id": message_id,
                "fragment_index": index,
                "fragment_count": fragments,
                "encoding": "base64",
                "data": BASE64.encode(bytes),
            }))
            .map_err(|_| {
                node_error(
                    "MUXIVA-CLIENT-EVENT-FRAGMENT",
                    "client event fragment serialization failed",
                )
            })?
        };
        if payload.len() > TRANSPORT_MESSAGE_LIMIT {
            return Err(node_error(
                "MUXIVA-CLIENT-EVENT-LIMIT",
                "encoded client event fragment exceeds the transport message limit",
            ));
        }
        messages.push(payload);
    }
    Ok(messages)
}

fn interval_tick_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([(
                "interval_ms",
                map([
                    ("type", Value::String("integer".into())),
                    ("minimum", Value::Integer(1)),
                    ("maximum", Value::Integer(60_000)),
                    ("default", Value::Integer(20)),
                ]),
            )]),
        ),
        (
            "required",
            Value::List(vec![Value::String("interval_ms".into())].into_boxed_slice()),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

struct IntervalTickFactory;

impl NodeFactory for IntervalTickFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        match (config.len(), config.get("interval_ms")) {
            (1, Some(Value::Integer(value))) if (1..=60_000).contains(value) => Ok(()),
            _ => Err(config_error(
                "interval tick requires interval_ms from 1 through 60000",
            )),
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let Some(Value::Integer(value)) = config.get("interval_ms") else {
            return Err(config_error("validated interval_ms is unavailable"));
        };
        Ok(Box::new(IntervalTick {
            interval: Duration::from_millis(*value as u64),
            sequence: 0,
        }))
    }
}

struct IntervalTick {
    interval: Duration,
    sequence: u64,
}

impl Node for IntervalTick {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        if input.is_some() {
            return Err(node_error(
                "MUXIVA-INTERVAL-TICK-INPUT",
                "interval tick source received input",
            ));
        }
        self.sequence = self.sequence.checked_add(1).ok_or_else(|| {
            node_error("MUXIVA-INTERVAL-TICK-SEQUENCE", "tick sequence overflowed")
        })?;
        let payload = EventData::new(
            NamespacedName::new("muxiva.runtime.tick")?,
            SchemaVersion::new(1)?,
            context.node_id().clone(),
            Value::Integer(i64::try_from(self.sequence).map_err(|_| {
                node_error(
                    "MUXIVA-INTERVAL-TICK-SEQUENCE",
                    "tick sequence cannot be represented as an event value",
                )
            })?),
        );
        let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
        let frame = Frame::new(
            FrameHeader::new(
                FrameId::new(format!("builtin-interval-tick-{serial}"))
                    .expect("bounded tick frame ID"),
                Timestamp::from_nanos(0),
                ClockDomain::new(
                    ClockDomainId::new("muxiva.runtime.interval").expect("valid tick clock"),
                    ClockKind::Monotonic,
                ),
                SequenceId::new(self.sequence),
                StreamId::new(format!("interval-{}", context.node_id()))
                    .expect("bounded tick stream"),
                TraceId::new(format!("interval-{}", context.node_id()))
                    .expect("bounded tick trace"),
                FrameType::Event,
                Metadata::empty(),
                Extensions::empty(),
                Lineage::empty(),
            )?,
            FramePayload::Event(payload),
        )?;
        context.emit(PortName::new("tick_out").unwrap(), frame)?;
        context.schedule_next_tick(self.interval);
        Ok(())
    }
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
            typed_descriptor(
                node_type,
                kind,
                &ports,
                if node_type == DEMO_MICROPHONE {
                    demo_microphone_schema()
                } else {
                    empty_schema()
                },
            ),
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
        if matches!(self.0, DemoNodeKind::Microphone) {
            let turns = match config.get("turns") {
                Some(Value::Integer(value)) if (1..=100).contains(value) => *value as usize,
                _ => {
                    return Err(config_error(
                        "demo microphone `turns` must be an integer from 1 through 100",
                    ))
                }
            };
            let interval = match config.get("interval_ms") {
                Some(Value::Integer(value)) if (0..=10_000).contains(value) => *value as u64,
                _ => {
                    return Err(config_error(
                        "demo microphone `interval_ms` must be an integer from 0 through 10000",
                    ))
                }
            };
            let _ = (turns, interval);
            Ok(())
        } else {
            validate_empty_config(config, "demo node")
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(match self.0 {
            DemoNodeKind::Microphone => Box::new(DemoMicrophone {
                turn: 0,
                turns: match config.get("turns") {
                    Some(Value::Integer(value)) => *value as usize,
                    _ => 1,
                },
                interval: Duration::from_millis(match config.get("interval_ms") {
                    Some(Value::Integer(value)) => *value as u64,
                    _ => 0,
                }),
            }) as Box<dyn Node>,
            DemoNodeKind::Asr => Box::new(DemoAsr),
            DemoNodeKind::Vad => Box::new(DemoVad),
            DemoNodeKind::Fusion => Box::new(DemoContextFusion::default()),
            DemoNodeKind::Llm => Box::new(DemoLlm),
            DemoNodeKind::Tts => Box::new(DemoTts),
            DemoNodeKind::Speaker => Box::new(DemoSpeaker),
        })
    }
}

struct DemoMicrophone {
    turn: usize,
    turns: usize,
    interval: Duration,
}
impl Node for DemoMicrophone {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        if input.is_some() {
            return Err(node_error(
                "MUXIVA-DEMO-SOURCE-INPUT",
                "demo microphone received input",
            ));
        }
        self.turn += 1;
        println!(
            "[MUXIVA][TURN][started] turn={} of={}",
            self.turn, self.turns
        );
        println!("[MUXIVA][FRAME][{}] turn={} audio=pcm_s16le rate_hz=16000 channels=1 duration_ms=20 provider=mock", context.node_id(), self.turn);
        context.emit(
            PortName::new("audio_out").unwrap(),
            demo_audio_frame("microphone", self.turn as u64)?,
        )?;
        if self.turn < self.turns {
            context.schedule_next_tick(self.interval);
        }
        Ok(())
    }
}

struct DemoAsr;
impl Node for DemoAsr {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "streaming ASR requires audio")?;
        let turn = input.header().sequence_id().get() as usize;
        let transcript = demo_transcript(turn);
        println!(
            "[MUXIVA][NODE][{}] transcript=\"{transcript}\" provider=mock",
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
    ) -> muxiva_types::Result<()> {
        let input = required_type(
            input,
            FrameType::Audio,
            "voice activity detector requires audio",
        )?;
        println!(
            "[MUXIVA][NODE][{}] speech_detected=true confidence=0.98 provider=mock",
            context.node_id()
        );
        let event = EventData::new(
            NamespacedName::new("muxiva.demo.speech.detected")?,
            SchemaVersion::new(1)?,
            context.node_id().clone(),
            Value::Bool(true),
        );
        let event_frame =
            derive_payload(&input, context.node_id(), "vad", FramePayload::Event(event))?;
        let published =
            context.publish_notification(event_frame.as_event().expect("event payload").clone())?;
        println!("[MUXIVA][NOTIFICATION-BUS][publish] topic=muxiva.demo.speech.detected turn={} subscribers={} enqueued={}", input.header().sequence_id().get(), published.matched, published.enqueued);
        let signal_frame = derive_payload(
            &input,
            context.node_id(),
            "vad-control",
            FramePayload::Signal(SignalData::new(
                NamespacedName::new("muxiva.voice.speech.started")?,
                SchemaVersion::new(1)?,
                context.node_id().clone(),
                Value::Integer(input.header().sequence_id().get() as i64),
            )),
        )?;
        context.emit_signal(signal_frame.as_signal().expect("signal payload").clone())?;
        println!(
            "[MUXIVA][SIGNAL][emit] name=muxiva.voice.speech.started turn={} route=downstream",
            input.header().sequence_id().get()
        );
        context.emit(PortName::new("speech_out").unwrap(), event_frame)?;
        Ok(())
    }
}

#[derive(Default)]
struct DemoContextFusion {
    transcripts: BTreeMap<u64, Box<str>>,
    speech: BTreeSet<u64>,
    emitted: BTreeSet<u64>,
}
impl Node for DemoContextFusion {
    fn on_signal(
        &mut self,
        signal: muxiva_types::SignalFrame,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        println!(
            "[MUXIVA][SIGNAL][received] node={} name={} turn={} action=observe-barge-in",
            context.node_id(),
            signal.data().name(),
            signal.header().sequence_id().get()
        );
        Ok(())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error("MUXIVA-DEMO-INPUT-MISSING", "context fusion requires input")
        })?;
        let turn = input.header().sequence_id().get();
        match context.input_port().map(PortName::as_str) {
            Some("transcript_in") => {
                self.transcripts.insert(
                    turn,
                    input
                        .as_text()
                        .ok_or_else(|| {
                            node_error("MUXIVA-DEMO-INPUT-TYPE", "transcript input must be text")
                        })?
                        .data()
                        .as_str()
                        .into(),
                );
            }
            Some("speech_in") => {
                input.ensure_type(FrameType::Event)?;
                self.speech.insert(turn);
            }
            _ => {
                return Err(node_error(
                    "MUXIVA-DEMO-INPUT-PORT",
                    "context fusion received an unknown port",
                ))
            }
        }
        if self.speech.contains(&turn)
            && self.transcripts.contains_key(&turn)
            && !self.emitted.contains(&turn)
        {
            self.emitted.insert(turn);
            let transcript = self.transcripts.get(&turn).unwrap();
            println!(
                "[MUXIVA][JOIN][{}] turn={turn} inputs=transcript+speech_event status=ready",
                context.node_id(),
            );
            let prompt = format!("turn={turn}; speech=true; user={transcript}");
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
    ) -> muxiva_types::Result<()> {
        let input = required_type(input, FrameType::Text, "reasoning LLM requires context")?;
        let turn = input.header().sequence_id().get() as usize;
        let response = demo_response(turn);
        println!(
            "[MUXIVA][NODE][{}] tokens_in=14 tokens_out=18 provider=mock",
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
    ) -> muxiva_types::Result<()> {
        let input = required_type(input, FrameType::Text, "neural TTS requires text")?;
        println!(
            "[MUXIVA][NODE][{}] voice=alloy audio=pcm_s16le duration_ms=20 provider=mock",
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
    ) -> muxiva_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "speaker requires audio")?;
        let duration_ms = input.as_audio().unwrap().data().duration_ns() / 1_000_000;
        println!(
            "[MUXIVA][RESULT][{}] played_audio_ms={duration_ms} provider=mock",
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

fn demo_microphone_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([
                (
                    "turns",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(100)),
                        ("default", Value::Integer(4)),
                    ]),
                ),
                (
                    "interval_ms",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(0)),
                        ("maximum", Value::Integer(10_000)),
                        ("default", Value::Integer(650)),
                    ]),
                ),
            ]),
        ),
        (
            "required",
            Value::List(
                vec![
                    Value::String("turns".into()),
                    Value::String("interval_ms".into()),
                ]
                .into_boxed_slice(),
            ),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Map(ValueMap::try_from_iter(entries).expect("valid built-in schema"))
}

fn audio_resample_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([
                (
                    "sample_rate_hz",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(8_000)),
                        ("maximum", Value::Integer(192_000)),
                    ]),
                ),
                ("input", audio_format_schema()),
                ("output", audio_format_schema()),
            ]),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

fn audio_format_schema() -> Value {
    map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([
                (
                    "sample_rate_hz",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(8_000)),
                        ("maximum", Value::Integer(192_000)),
                    ]),
                ),
                (
                    "channels",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(32)),
                    ]),
                ),
                (
                    "sample_format",
                    map([("type", Value::String("string".into()))]),
                ),
            ]),
        ),
        ("additionalProperties", Value::Bool(false)),
    ])
}

fn audio_vad_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([
                (
                    "threshold",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(32_767)),
                        ("default", Value::Integer(600)),
                    ]),
                ),
                (
                    "start_frames",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(100)),
                        ("default", Value::Integer(2)),
                    ]),
                ),
                (
                    "stop_frames",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(500)),
                        ("default", Value::Integer(18)),
                    ]),
                ),
            ]),
        ),
        (
            "required",
            Value::List(
                vec![
                    Value::String("threshold".into()),
                    Value::String("start_frames".into()),
                    Value::String("stop_frames".into()),
                ]
                .into_boxed_slice(),
            ),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

struct AudioVadFactory;

impl NodeFactory for AudioVadFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        let valid = matches!(config.get("threshold"), Some(Value::Integer(1..=32_767)))
            && matches!(config.get("start_frames"), Some(Value::Integer(1..=100)))
            && matches!(config.get("stop_frames"), Some(Value::Integer(1..=500)))
            && config.len() == 3;
        if valid {
            Ok(())
        } else {
            Err(config_error(
                "audio VAD requires threshold, start_frames, and stop_frames in range",
            ))
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let integer = |name: &str| match config.get(name) {
            Some(Value::Integer(value)) => *value as u32,
            _ => 0,
        };
        Ok(Box::new(AudioVad {
            threshold: integer("threshold") as u64,
            start_frames: integer("start_frames"),
            stop_frames: integer("stop_frames"),
            loud_frames: 0,
            quiet_frames: 0,
            active: false,
        }))
    }
}

struct AudioVad {
    threshold: u64,
    start_frames: u32,
    stop_frames: u32,
    loud_frames: u32,
    quiet_frames: u32,
    active: bool,
}

impl AudioVad {
    fn transition(
        &mut self,
        active: bool,
        input: &Frame,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.active = active;
        let topic = if active {
            "muxiva.voice.speech.started"
        } else {
            "muxiva.voice.speech.stopped"
        };
        let event = derive_payload(
            input,
            context.node_id(),
            "audio-vad",
            FramePayload::Event(EventData::new(
                NamespacedName::new(topic)?,
                SchemaVersion::new(1)?,
                context.node_id().clone(),
                Value::Bool(active),
            )),
        )?;
        context.publish_notification(event.as_event().expect("event payload").clone())?;
        context.emit(PortName::new("speech_out").unwrap(), event)?;
        if active {
            let signal = derive_payload(
                input,
                context.node_id(),
                "audio-vad-interrupt",
                FramePayload::Signal(SignalData::new(
                    NamespacedName::new("muxiva.voice.speech.started")?,
                    SchemaVersion::new(1)?,
                    context.node_id().clone(),
                    Value::String("barge-in".into()),
                )),
            )?;
            context.emit_signal(signal.as_signal().expect("signal payload").clone())?;
        }
        println!(
            "[MUXIVA][VOICE][{}] state={} action={}",
            context.node_id(),
            if active { "speaking" } else { "listening" },
            if active {
                "interrupt-downstream"
            } else {
                "close-turn"
            }
        );
        Ok(())
    }
}

impl Node for AudioVad {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "audio VAD requires audio")?;
        let audio = input.as_audio().expect("validated audio").data();
        if audio.sample_format() != PcmSampleFormat::I16Le
            || audio.layout() != AudioLayout::Interleaved
        {
            return Err(node_error(
                "MUXIVA-AUDIO-VAD-FORMAT",
                "audio VAD requires interleaved PCM s16le",
            ));
        }
        let samples = audio.buffer().as_slice().chunks_exact(2);
        let (sum, count) = samples.fold((0_u64, 0_u64), |(sum, count), bytes| {
            let sample = i64::from(i16::from_le_bytes([bytes[0], bytes[1]])).unsigned_abs();
            (sum.saturating_add(sample), count + 1)
        });
        let loud = count > 0 && sum / count >= self.threshold;
        if loud {
            self.loud_frames = self.loud_frames.saturating_add(1);
            self.quiet_frames = 0;
        } else {
            self.quiet_frames = self.quiet_frames.saturating_add(1);
            self.loud_frames = 0;
        }
        if !self.active && self.loud_frames >= self.start_frames {
            self.transition(true, &input, context)?;
        } else if self.active && self.quiet_frames >= self.stop_frames {
            self.transition(false, &input, context)?;
        }
        Ok(())
    }
}

struct VoiceTurnContextFactory;

impl NodeFactory for VoiceTurnContextFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        validate_empty_config(config, "voice turn context")
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(VoiceTurnContext::default()))
    }
}

#[derive(Default)]
struct VoiceTurnContext {
    speech_active: bool,
    pending_transcript: Option<Box<str>>,
}

impl VoiceTurnContext {
    fn emit_pending(
        &mut self,
        input: &Frame,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        if let Some(transcript) = self.pending_transcript.take() {
            context.emit(
                PortName::new("context_out").unwrap(),
                derive_payload(
                    input,
                    context.node_id(),
                    "voice-turn-context",
                    FramePayload::Text(TextData::new(transcript)),
                )?,
            )?;
        }
        Ok(())
    }
}

impl Node for VoiceTurnContext {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "MUXIVA-VOICE-CONTEXT-INPUT",
                "voice turn context requires input",
            )
        })?;
        match context.input_port().map(PortName::as_str) {
            Some("transcript_in") => {
                let transcript = input
                    .as_text()
                    .ok_or_else(|| {
                        node_error("MUXIVA-VOICE-CONTEXT-TYPE", "transcript must be text")
                    })?
                    .data()
                    .as_str()
                    .trim();
                if !transcript.is_empty() {
                    self.pending_transcript = Some(transcript.into());
                    if !self.speech_active {
                        self.emit_pending(&input, context)?;
                    }
                }
            }
            Some("speech_in") => {
                let event = input.as_event().ok_or_else(|| {
                    node_error("MUXIVA-VOICE-CONTEXT-TYPE", "speech input must be an event")
                })?;
                match event.data().topic().as_str() {
                    "muxiva.voice.speech.started" => {
                        self.speech_active = true;
                        self.pending_transcript = None;
                    }
                    "muxiva.voice.speech.stopped" => {
                        self.speech_active = false;
                        self.emit_pending(&input, context)?;
                    }
                    _ => {}
                }
            }
            _ => {
                return Err(node_error(
                    "MUXIVA-VOICE-CONTEXT-PORT",
                    "voice turn context received an unknown port",
                ))
            }
        }
        Ok(())
    }
}

struct AudioResampleFactory;

impl NodeFactory for AudioResampleFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        parse_resampler_config(config).map(|_| ())
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let parsed = parse_resampler_config(config)?;
        Ok(Box::new(AudioResample {
            input_rate_hz: parsed.input_rate_hz,
            input_channels: parsed.input_channels,
            target_rate_hz: parsed.output_rate_hz,
            target_channels: parsed.output_channels,
        }))
    }
}

struct ResamplerConfig {
    input_rate_hz: Option<u32>,
    input_channels: Option<u16>,
    output_rate_hz: u32,
    output_channels: Option<u16>,
}

fn parse_resampler_config(config: &ConfigMap) -> Result<ResamplerConfig, NodeFactoryError> {
    if let Some(Value::Integer(rate)) = config.get("sample_rate_hz") {
        if config.len() == 1 && (8_000..=192_000).contains(rate) {
            return Ok(ResamplerConfig {
                input_rate_hz: None,
                input_channels: None,
                output_rate_hz: *rate as u32,
                output_channels: None,
            });
        }
    }
    if config
        .iter()
        .any(|(key, _)| key.as_str() != "input" && key.as_str() != "output")
    {
        return Err(config_error(
            "audio resampler accepts `input` and `output` format objects",
        ));
    }
    let input = match config.get("input") {
        Some(Value::Map(value)) => Some(value),
        None => None,
        _ => return Err(config_error("audio resampler `input` must be an object")),
    };
    let output = match config.get("output") {
        Some(Value::Map(value)) => value,
        _ => return Err(config_error("audio resampler requires an `output` object")),
    };
    let rate = |value: Option<&Value>| match value {
        Some(Value::Integer(value)) if (8_000..=192_000).contains(value) => Ok(*value as u32),
        None => Err(config_error("audio format requires `sample_rate_hz`")),
        _ => Err(config_error(
            "sample_rate_hz must be an integer from 8000 through 192000",
        )),
    };
    let channels = |value: Option<&Value>| match value {
        Some(Value::Integer(value)) if (1..=32).contains(value) => Ok(Some(*value as u16)),
        None => Ok(None),
        _ => Err(config_error(
            "channels must be an integer from 1 through 32",
        )),
    };
    for format in input.into_iter().chain([output]) {
        if let Some(Value::String(value)) = format.get("sample_format") {
            if value.as_ref() != "pcm_s16le" {
                return Err(config_error("audio resampler currently supports pcm_s16le"));
            }
        }
    }
    Ok(ResamplerConfig {
        input_rate_hz: input
            .map(|value| rate(value.get("sample_rate_hz")))
            .transpose()?,
        input_channels: input
            .map(|value| channels(value.get("channels")))
            .transpose()?
            .flatten(),
        output_rate_hz: rate(output.get("sample_rate_hz"))?,
        output_channels: channels(output.get("channels"))?,
    })
}

struct AudioResample {
    input_rate_hz: Option<u32>,
    input_channels: Option<u16>,
    target_rate_hz: u32,
    target_channels: Option<u16>,
}

impl Node for AudioResample {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "audio resampler requires audio")?;
        let source = input.as_audio().expect("validated audio").data();
        if source.sample_format() != PcmSampleFormat::I16Le
            || source.layout() != AudioLayout::Interleaved
        {
            return Err(node_error(
                "MUXIVA-AUDIO-RESAMPLE-FORMAT",
                "audio resampler requires interleaved PCM s16le",
            ));
        }
        if self
            .input_rate_hz
            .is_some_and(|value| value != source.sample_rate_hz())
            || self
                .input_channels
                .is_some_and(|value| value != source.channels())
            || self
                .target_channels
                .is_some_and(|value| value != source.channels())
        {
            return Err(node_error(
                "MUXIVA-AUDIO-RESAMPLE-CONTRACT",
                "audio frame does not match the configured input/output channel contract",
            ));
        }
        let payload = if source.sample_rate_hz() == self.target_rate_hz {
            source.clone()
        } else {
            resample_pcm16(source, self.target_rate_hz)?
        };
        let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
        let output = input.derive(
            FrameDerivation::new(
                FrameId::new(format!("builtin-audio-resample-{serial}")).expect("bounded frame ID"),
                input.header().timestamp(),
                input.header().sequence_id(),
                TransformOrigin::new(Some(context.node_id().clone()), None)?,
                "builtin_audio_resample",
            )?
            .with_payload(FramePayload::Audio(payload)),
        )?;
        context.emit(PortName::new("audio_out").unwrap(), output)?;
        Ok(())
    }
}

fn resample_pcm16(source: &AudioData, target_rate_hz: u32) -> muxiva_types::Result<AudioData> {
    let source_samples = source.samples_per_channel();
    let target_samples = source_samples
        .checked_mul(u64::from(target_rate_hz))
        .and_then(|value| value.checked_add(u64::from(source.sample_rate_hz()) / 2))
        .map(|value| value / u64::from(source.sample_rate_hz()))
        .ok_or_else(|| {
            node_error(
                "MUXIVA-AUDIO-RESAMPLE-SIZE",
                "resampled audio size overflow",
            )
        })?;
    if target_samples == 0 {
        return Err(node_error(
            "MUXIVA-AUDIO-RESAMPLE-SIZE",
            "resampled audio would contain no samples",
        ));
    }
    let channels = usize::from(source.channels());
    let source_values = source
        .buffer()
        .as_slice()
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    let capacity = usize::try_from(target_samples)
        .ok()
        .and_then(|samples| samples.checked_mul(channels))
        .and_then(|samples| samples.checked_mul(2))
        .ok_or_else(|| {
            node_error(
                "MUXIVA-AUDIO-RESAMPLE-SIZE",
                "resampled audio size overflow",
            )
        })?;
    let mut output = Vec::with_capacity(capacity);
    for target_index in 0..target_samples {
        let numerator = target_index * u64::from(source.sample_rate_hz());
        let left = (numerator / u64::from(target_rate_hz)).min(source_samples - 1);
        let right = (left + 1).min(source_samples - 1);
        let fraction = (numerator % u64::from(target_rate_hz)) as f64 / f64::from(target_rate_hz);
        for channel in 0..channels {
            let left_index = usize::try_from(left).unwrap() * channels + channel;
            let right_index = usize::try_from(right).unwrap() * channels + channel;
            let value = f64::from(source_values[left_index]) * (1.0 - fraction)
                + f64::from(source_values[right_index]) * fraction;
            output.extend_from_slice(&(value.round() as i16).to_le_bytes());
        }
    }
    AudioData::new(
        FrameBuffer::from_vec(output),
        target_rate_hz,
        source.channels(),
        PcmSampleFormat::I16Le,
        AudioLayout::Interleaved,
        target_samples,
    )
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
    ) -> muxiva_types::Result<()> {
        if input.is_some() {
            return Err(node_error(
                "MUXIVA-BUILTIN-SOURCE-INPUT",
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
    ) -> muxiva_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "MUXIVA-BUILTIN-INPUT-MISSING",
                "uppercase transform requires a text input",
            )
        })?;
        let text = input.as_text().ok_or_else(|| {
            node_error(
                "MUXIVA-BUILTIN-INPUT-TYPE",
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
    ) -> muxiva_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "MUXIVA-BUILTIN-INPUT-MISSING",
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
    ) -> muxiva_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "MUXIVA-BUILTIN-INPUT-MISSING",
                "stdout text sink requires a text input",
            )
        })?;
        let text = input.as_text().ok_or_else(|| {
            node_error(
                "MUXIVA-BUILTIN-INPUT-TYPE",
                "stdout text sink requires a text frame",
            )
        })?;
        println!(
            "[MUXIVA][RESULT][{}] {}",
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
    NodeFactoryError::new("MUXIVA-BUILTIN-CONFIG", message)
}

fn node_error(code: &'static str, message: &'static str) -> MuxivaError {
    MuxivaError::new(ErrorCategory::Internal, code, message)
}

fn source_frame(text: &str) -> muxiva_types::Result<Frame> {
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    Frame::new(
        FrameHeader::new(
            FrameId::new(format!("builtin-source-{serial}")).expect("bounded built-in frame ID"),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("muxiva.builtin.text").expect("valid built-in clock"),
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

fn demo_audio_frame(origin: &str, sequence: u64) -> muxiva_types::Result<Frame> {
    let audio = AudioData::new(
        FrameBuffer::from_vec(vec![0; 640]),
        16_000,
        1,
        PcmSampleFormat::I16Le,
        AudioLayout::Interleaved,
        320,
    )?;
    demo_frame(origin, sequence, FramePayload::Audio(audio))
}

fn demo_frame(origin: &str, sequence: u64, payload: FramePayload) -> muxiva_types::Result<Frame> {
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    let frame_type = payload.frame_type();
    Frame::new(
        FrameHeader::new(
            FrameId::new(format!("demo-{origin}-{serial}")).expect("bounded demo frame ID"),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("muxiva.demo.media").expect("valid demo clock"),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(sequence),
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

fn demo_transcript(turn: usize) -> &'static str {
    const TRANSCRIPTS: &[&str] = &[
        "Hello Muxiva, what can this runtime do?",
        "Can I interrupt while the assistant is speaking?",
        "How do nodes communicate without returning a frame?",
        "Great, summarize this live session for me.",
    ];
    TRANSCRIPTS[(turn.saturating_sub(1)) % TRANSCRIPTS.len()]
}

fn demo_response(turn: usize) -> &'static str {
    const RESPONSES: &[&str] = &[
        "I run audio, ASR, VAD, reasoning, and TTS as one typed concurrent graph.",
        "Yes. Signals travel on the graph control plane so a new turn can interrupt downstream work.",
        "Use ctx.emit for Graph data, ctx.emit_signal for adjacent control, and ctx.publish_notification for the process-local NotificationBus.",
        "This session completed four voice turns while preserving typed routing, control, and observable events.",
    ];
    RESPONSES[(turn.saturating_sub(1)) % RESPONSES.len()]
}

fn required_type(
    input: Option<Frame>,
    frame_type: FrameType,
    message: &'static str,
) -> muxiva_types::Result<Frame> {
    let input = input.ok_or_else(|| node_error("MUXIVA-DEMO-INPUT-MISSING", message))?;
    input.ensure_type(frame_type)?;
    Ok(input)
}

fn derive_payload(
    input: &Frame,
    node_id: &NodeId,
    reason: &'static str,
    payload: FramePayload,
) -> muxiva_types::Result<Frame> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_messages_respect_transport_limits_and_round_trip_fragments() {
        let source = vec![b'x'; 2_000];
        let messages = client_message_payloads("frame-1", &source).unwrap();
        assert_eq!(messages.len(), 4);
        assert!(messages.iter().all(|message| message.len() <= 1_024));

        let mut restored = Vec::new();
        for (expected_index, message) in messages.iter().enumerate() {
            let envelope: serde_json::Value = serde_json::from_slice(message).unwrap();
            assert_eq!(envelope["version"], "muxiva.transport-fragment/v1");
            assert_eq!(envelope["message_id"], "frame-1");
            assert_eq!(envelope["fragment_index"], expected_index);
            restored.extend(BASE64.decode(envelope["data"].as_str().unwrap()).unwrap());
        }
        assert_eq!(restored, source);

        assert!(client_message_payloads("frame-2", &vec![0; 32 * 1_024 + 1]).is_err());
    }

    #[test]
    fn pcm16_resampler_preserves_duration_in_both_demo_directions() {
        let source = AudioData::new(
            FrameBuffer::from_vec(vec![0; 1_920]),
            48_000,
            1,
            PcmSampleFormat::I16Le,
            AudioLayout::Interleaved,
            960,
        )
        .unwrap();
        let at_16k = resample_pcm16(&source, 16_000).unwrap();
        assert_eq!(at_16k.samples_per_channel(), 320);
        assert_eq!(at_16k.duration_ns(), 20_000_000);

        let provider_output = AudioData::new(
            FrameBuffer::from_vec(vec![0; 960]),
            24_000,
            1,
            PcmSampleFormat::I16Le,
            AudioLayout::Interleaved,
            480,
        )
        .unwrap();
        let at_48k = resample_pcm16(&provider_output, 48_000).unwrap();
        assert_eq!(at_48k.samples_per_channel(), 960);
        assert_eq!(at_48k.duration_ns(), 20_000_000);
    }
}
