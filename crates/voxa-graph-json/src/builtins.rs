use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
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
    SignalData, StreamId, TextData, Timestamp, TraceId, TransformOrigin, Value, ValueMap,
    VoxaError,
};

pub const BUILTIN_FACTORY_VERSION: &str = "1.0.0";
pub const TEXT_SOURCE: &str = "builtin.text_source";
pub const UPPERCASE: &str = "builtin.uppercase";
pub const TEXT_SINK: &str = "builtin.text_sink";
pub const STDOUT_TEXT_SINK: &str = "builtin.stdout_text_sink";
pub const AUDIO_RESAMPLE: &str = "builtin.audio_resample";
pub const INTERVAL_TICK: &str = "builtin.interval_tick";
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
            AUDIO_RESAMPLE,
            NodeKind::Transform,
            &[
                ("audio_in", PortDirection::Input, FrameType::Audio),
                ("audio_out", PortDirection::Output, FrameType::Audio),
            ],
            audio_resample_schema(),
        ),
        Arc::new(AudioResampleFactory),
    );
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
    ) -> voxa_types::Result<()> {
        if input.is_some() {
            return Err(node_error(
                "VOXA-INTERVAL-TICK-INPUT",
                "interval tick source received input",
            ));
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| node_error("VOXA-INTERVAL-TICK-SEQUENCE", "tick sequence overflowed"))?;
        let payload = EventData::new(
            NamespacedName::new("voxa.runtime.tick")?,
            SchemaVersion::new(1)?,
            context.node_id().clone(),
            Value::Integer(i64::try_from(self.sequence).map_err(|_| {
                node_error(
                    "VOXA-INTERVAL-TICK-SEQUENCE",
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
                    ClockDomainId::new("voxa.runtime.interval").expect("valid tick clock"),
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
    ) -> voxa_types::Result<()> {
        if input.is_some() {
            return Err(node_error(
                "VOXA-DEMO-SOURCE-INPUT",
                "demo microphone received input",
            ));
        }
        self.turn += 1;
        println!("[VOXA][TURN][started] turn={} of={}", self.turn, self.turns);
        println!("[VOXA][FRAME][{}] turn={} audio=pcm_s16le rate_hz=16000 channels=1 duration_ms=20 provider=mock", context.node_id(), self.turn);
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
    ) -> voxa_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "streaming ASR requires audio")?;
        let turn = input.header().sequence_id().get() as usize;
        let transcript = demo_transcript(turn);
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
        let event_frame =
            derive_payload(&input, context.node_id(), "vad", FramePayload::Event(event))?;
        let published =
            context.publish_event(event_frame.as_event().expect("event payload").clone())?;
        println!("[VOXA][EVENTBUS][publish] topic=voxa.demo.speech.detected turn={} subscribers={} enqueued={}", input.header().sequence_id().get(), published.matched, published.enqueued);
        let signal_frame = derive_payload(
            &input,
            context.node_id(),
            "vad-control",
            FramePayload::Signal(SignalData::new(
                NamespacedName::new("voxa.voice.speech.started")?,
                SchemaVersion::new(1)?,
                context.node_id().clone(),
                Value::Integer(input.header().sequence_id().get() as i64),
            )),
        )?;
        context.emit_signal(signal_frame.as_signal().expect("signal payload").clone())?;
        println!(
            "[VOXA][SIGNAL][emit] name=voxa.voice.speech.started turn={} route=downstream",
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
        signal: voxa_types::SignalFrame,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        println!(
            "[VOXA][SIGNAL][received] node={} name={} turn={} action=observe-barge-in",
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
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error("VOXA-DEMO-INPUT-MISSING", "context fusion requires input")
        })?;
        let turn = input.header().sequence_id().get();
        match context.input_port().map(PortName::as_str) {
            Some("transcript_in") => {
                self.transcripts.insert(
                    turn,
                    input
                        .as_text()
                        .ok_or_else(|| {
                            node_error("VOXA-DEMO-INPUT-TYPE", "transcript input must be text")
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
                    "VOXA-DEMO-INPUT-PORT",
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
                "[VOXA][JOIN][{}] turn={turn} inputs=transcript+speech_event status=ready",
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
    ) -> voxa_types::Result<()> {
        let input = required_type(input, FrameType::Text, "reasoning LLM requires context")?;
        let turn = input.header().sequence_id().get() as usize;
        let response = demo_response(turn);
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
            map([(
                "sample_rate_hz",
                map([
                    ("type", Value::String("integer".into())),
                    ("minimum", Value::Integer(8_000)),
                    ("maximum", Value::Integer(192_000)),
                    ("default", Value::Integer(16_000)),
                ]),
            )]),
        ),
        (
            "required",
            Value::List(vec![Value::String("sample_rate_hz".into())].into_boxed_slice()),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

struct AudioResampleFactory;

impl NodeFactory for AudioResampleFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if config.len() != 1 {
            return Err(config_error(
                "audio resampler accepts exactly `sample_rate_hz`",
            ));
        }
        match config.get("sample_rate_hz") {
            Some(Value::Integer(rate)) if (8_000..=192_000).contains(rate) => Ok(()),
            _ => Err(config_error(
                "audio resampler sample_rate_hz must be an integer from 8000 through 192000",
            )),
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let Some(Value::Integer(rate)) = config.get("sample_rate_hz") else {
            return Err(config_error("validated sample_rate_hz is unavailable"));
        };
        Ok(Box::new(AudioResample {
            target_rate_hz: *rate as u32,
        }))
    }
}

struct AudioResample {
    target_rate_hz: u32,
}

impl Node for AudioResample {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = required_type(input, FrameType::Audio, "audio resampler requires audio")?;
        let source = input.as_audio().expect("validated audio").data();
        if source.sample_format() != PcmSampleFormat::I16Le
            || source.layout() != AudioLayout::Interleaved
        {
            return Err(node_error(
                "VOXA-AUDIO-RESAMPLE-FORMAT",
                "audio resampler requires interleaved PCM s16le",
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

    fn on_signal(
        &mut self,
        signal: voxa_types::SignalFrame,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let payload = SignalData::new(
            signal.data().name().clone(),
            signal.data().schema_version(),
            context.node_id().clone(),
            signal.data().payload().clone(),
        );
        let parent = Frame::Signal(signal);
        let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
        let forwarded = parent.derive(
            FrameDerivation::new(
                FrameId::new(format!("builtin-audio-resample-signal-{serial}"))
                    .expect("bounded frame ID"),
                parent.header().timestamp(),
                parent.header().sequence_id(),
                TransformOrigin::new(Some(context.node_id().clone()), None)?,
                "builtin_audio_resample_signal",
            )?
            .with_payload(FramePayload::Signal(payload)),
        )?;
        let Frame::Signal(forwarded) = forwarded else {
            unreachable!("signal payload creates signal frame");
        };
        context.emit_signal(forwarded)?;
        Ok(())
    }
}

fn resample_pcm16(source: &AudioData, target_rate_hz: u32) -> voxa_types::Result<AudioData> {
    let source_samples = source.samples_per_channel();
    let target_samples = source_samples
        .checked_mul(u64::from(target_rate_hz))
        .and_then(|value| value.checked_add(u64::from(source.sample_rate_hz()) / 2))
        .map(|value| value / u64::from(source.sample_rate_hz()))
        .ok_or_else(|| node_error("VOXA-AUDIO-RESAMPLE-SIZE", "resampled audio size overflow"))?;
    if target_samples == 0 {
        return Err(node_error(
            "VOXA-AUDIO-RESAMPLE-SIZE",
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
        .ok_or_else(|| node_error("VOXA-AUDIO-RESAMPLE-SIZE", "resampled audio size overflow"))?;
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

fn demo_audio_frame(origin: &str, sequence: u64) -> voxa_types::Result<Frame> {
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

fn demo_frame(origin: &str, sequence: u64, payload: FramePayload) -> voxa_types::Result<Frame> {
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
        "Hello Voxa, what can this runtime do?",
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
        "Use ctx.emit for data, ctx.emit_signal for graph control, and ctx.publish_event for the global EventBus.",
        "This session completed four voice turns while preserving typed routing, control, and observable events.",
    ];
    RESPONSES[(turn.saturating_sub(1)) % RESPONSES.len()]
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

#[cfg(test)]
mod tests {
    use super::*;

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
