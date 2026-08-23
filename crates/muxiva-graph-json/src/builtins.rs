use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, Read},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use muxiva_core::{
    ConfigMap, ConfigSchema, LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeFactory,
    NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration, NodeRegistry,
    NodeTypeName, PortDescriptor, PortDirection, PortName,
};
use muxiva_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, ErrorCategory, EventData,
    Extensions, Frame, FrameBuffer, FrameDerivation, FrameHeader, FrameId, FramePayload, FrameType,
    Lineage, Metadata, MuxivaError, NamespacedName, NodeId, PcmSampleFormat, SchemaVersion,
    SequenceId, SignalData, StreamId, TextData, Timestamp, TraceId, TransformOrigin, Value,
    ValueMap,
};

pub const BUILTIN_FACTORY_VERSION: &str = "1.0.0";
pub const TEXT_SOURCE: &str = "builtin.text_source";
pub const UPPERCASE: &str = "builtin.uppercase";
pub const TEXT_SINK: &str = "builtin.text_sink";
pub const STDOUT_TEXT_SINK: &str = "builtin.stdout_text_sink";
pub const SPEECH_FORMATTER: &str = "builtin.speech_formatter";
pub const AUDIO_RESAMPLER: &str = "builtin.audio_resampler";
pub const INTERVAL_TICK: &str = "builtin.interval_tick";
pub const AUDIO_VAD: &str = "builtin.audio_vad";
pub const VOICE_TURN_CONTEXT: &str = "builtin.voice_turn_context";
pub const VOICE_TURN_CONTROLLER: &str = "builtin.voice_turn_controller";
pub const TEXT_CANCELLATION_GATE: &str = "builtin.text_cancellation_gate";
pub const DEMO_MICROPHONE: &str = "builtin.demo.microphone";
pub const DEMO_STREAMING_ASR: &str = "builtin.demo.streaming_asr";
pub const DEMO_VOICE_ACTIVITY: &str = "builtin.demo.voice_activity";
pub const DEMO_CONTEXT_FUSION: &str = "builtin.demo.context_fusion";
pub const DEMO_REASONING_LLM: &str = "builtin.demo.reasoning_llm";
pub const DEMO_NEURAL_TTS: &str = "builtin.demo.neural_tts";
pub const DEMO_SPEAKER: &str = "builtin.demo.speaker";
pub const LLM_OPENAI_COMPATIBLE: &str = "builtin.llm_openai_compatible";
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
    register(
        &mut registry,
        typed_descriptor(
            SPEECH_FORMATTER,
            NodeKind::Transform,
            &[
                (TEXT_INPUT, PortDirection::Input, FrameType::Text),
                ("signal_in", PortDirection::Input, FrameType::Signal),
                (TEXT_OUTPUT, PortDirection::Output, FrameType::Text),
            ],
            speech_formatter_schema(),
        ),
        Arc::new(SpeechFormatterFactory),
    );
    register_demo_nodes(&mut registry);
    register_audio_nodes(&mut registry);
    register_llm_node(&mut registry);
    registry
}

fn register_audio_nodes(registry: &mut NodeRegistry) {
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
    register(
        registry,
        typed_descriptor(
            VOICE_TURN_CONTROLLER,
            NodeKind::Transform,
            &[
                ("transcript_in", PortDirection::Input, FrameType::Text),
                ("preview_in", PortDirection::Input, FrameType::Text),
                ("activity_in", PortDirection::Input, FrameType::Event),
                ("interrupt_in", PortDirection::Input, FrameType::Signal),
                ("prompt_out", PortDirection::Output, FrameType::Text),
                ("transcript_out", PortDirection::Output, FrameType::Text),
                ("activity_out", PortDirection::Output, FrameType::Event),
                ("event_out", PortDirection::Output, FrameType::Event),
                ("signal_out", PortDirection::Output, FrameType::Signal),
            ],
            voice_turn_controller_schema(),
        ),
        Arc::new(VoiceTurnControllerFactory),
    );
}

fn register_llm_node(registry: &mut NodeRegistry) {
    register(
        registry,
        typed_descriptor(
            LLM_OPENAI_COMPATIBLE,
            NodeKind::Transform,
            &[
                ("text_in", PortDirection::Input, FrameType::Text),
                ("signal_in", PortDirection::Input, FrameType::Signal),
                ("text_out", PortDirection::Output, FrameType::Text),
                ("event_out", PortDirection::Output, FrameType::Event),
            ],
            llm_openai_compatible_schema(),
        ),
        Arc::new(LlmOpenAiCompatibleFactory),
    );
}

fn llm_openai_compatible_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([
                (
                    "endpoint",
                    map([
                        ("type", Value::String("string".into())),
                        (
                            "description",
                            Value::String(
                                "OpenAI-compatible base URL without /chat/completions, e.g. https://api.deepseek.com/v1".into(),
                            ),
                        ),
                    ]),
                ),
                (
                    "api_key_env",
                    map([
                        ("type", Value::String("string".into())),
                        (
                            "description",
                            Value::String(
                                "Environment variable that holds the API key; empty means no Authorization header".into(),
                            ),
                        ),
                        ("default", Value::String("".into())),
                    ]),
                ),
                (
                    "model",
                    map([
                        ("type", Value::String("string".into())),
                        (
                            "description",
                            Value::String("Model name, e.g. deepseek-chat, gpt-4o-mini, qwen-flash".into()),
                        ),
                    ]),
                ),
                (
                    "system_prompt",
                    map([
                        ("type", Value::String("string".into())),
                        ("default", Value::String(LLM_DEFAULT_SYSTEM_PROMPT.into())),
                    ]),
                ),
                (
                    "temperature",
                    map([
                        ("type", Value::String("number".into())),
                        ("minimum", Value::Integer(0)),
                        ("maximum", Value::Integer(2)),
                        ("default", Value::Integer(0)),
                    ]),
                ),
                (
                    "max_tokens",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(32_768)),
                        ("default", Value::Integer(512)),
                    ]),
                ),
                (
                    "timeout_ms",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1_000)),
                        ("maximum", Value::Integer(300_000)),
                        ("default", Value::Integer(60_000)),
                    ]),
                ),
                (
                    "max_results_per_wakeup",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(256)),
                        ("default", Value::Integer(32)),
                    ]),
                ),
                (
                    "history_turns",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(0)),
                        ("maximum", Value::Integer(32)),
                        ("default", Value::Integer(6)),
                    ]),
                ),
                (
                    "sentence_chunk_characters",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(20)),
                        ("maximum", Value::Integer(400)),
                        ("default", Value::Integer(80)),
                    ]),
                ),
                (
                    "stream",
                    map([
                        ("type", Value::String("boolean".into())),
                        ("default", Value::Bool(true)),
                    ]),
                ),
            ]),
        ),
        (
            "required",
            Value::List(
                vec![Value::String("endpoint".into()), Value::String("model".into())]
                    .into_boxed_slice(),
            ),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

const LLM_DEFAULT_SYSTEM_PROMPT: &str =
    "You are Muxiva, a warm, concise real-time voice assistant. Respond in the user's language using short, natural spoken sentences. Never use Markdown, lists, or URLs aloud.";

struct LlmOpenAiCompatibleFactory;

impl NodeFactory for LlmOpenAiCompatibleFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if !matches!(config.get("endpoint"), Some(Value::String(value)) if !value.trim().is_empty() && value.len() <= 1024) {
            return Err(config_error("LLM node requires a non-empty endpoint string"));
        }
        if !matches!(config.get("model"), Some(Value::String(value)) if !value.trim().is_empty() && value.len() <= 256) {
            return Err(config_error("LLM node requires a non-empty model string"));
        }
        if !matches!(config.get("api_key_env"), Some(Value::String(value)) if value.len() <= 256) {
            return Err(config_error("LLM api_key_env must be a bounded string"));
        }
        if !matches!(config.get("system_prompt"), Some(Value::String(value)) if value.len() <= 16_384) {
            return Err(config_error("LLM system_prompt must be a bounded string"));
        }
        if !matches!(config.get("temperature"), Some(Value::Float(value)) if value.get() >= 0.0 && value.get() <= 2.0) {
            return Err(config_error("LLM temperature must be between 0 and 2"));
        }
        if !matches!(config.get("max_tokens"), Some(Value::Integer(1..=32_768)))
            || !matches!(config.get("timeout_ms"), Some(Value::Integer(1_000..=300_000)))
            || !matches!(config.get("max_results_per_wakeup"), Some(Value::Integer(1..=256)))
            || !matches!(config.get("history_turns"), Some(Value::Integer(0..=32)))
            || !matches!(config.get("sentence_chunk_characters"), Some(Value::Integer(20..=400)))
            || !matches!(config.get("stream"), Some(Value::Bool(_)))
        {
            return Err(config_error("LLM node has an out-of-range numeric or boolean value"));
        }
        Ok(())
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let string = |name: &str| match config.get(name) {
            Some(Value::String(value)) => value.to_string(),
            _ => String::new(),
        };
        let temperature = match config.get("temperature") {
            Some(Value::Float(value)) => value.get(),
            _ => 0.6,
        };
        let integer = |name: &str, default: i64| match config.get(name) {
            Some(Value::Integer(value)) => *value,
            _ => default,
        };
        let (sender, results) = mpsc::channel();
        Ok(Box::new(LlmOpenAiCompatible {
            endpoint: string("endpoint").trim_end_matches('/').to_owned(),
            api_key_env: {
                let value = string("api_key_env");
                if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                }
            },
            model: string("model"),
            system_prompt: string("system_prompt"),
            temperature,
            max_tokens: integer("max_tokens", 512) as u32,
            timeout_ms: integer("timeout_ms", 60_000) as u64,
            max_results_per_wakeup: integer("max_results_per_wakeup", 32) as usize,
            history_turns: integer("history_turns", 6) as usize,
            sentence_chunk_characters: integer("sentence_chunk_characters", 80) as usize,
            stream: matches!(config.get("stream"), Some(Value::Bool(true))),
            generation: 0,
            cancel: None,
            pending: Arc::new(AtomicUsize::new(0)),
            worker: None,
            sender,
            results,
            history: Vec::new(),
        }))
    }
}

enum LlmResult {
    Delta { generation: u64, sequence: u64, text: String },
    Done { generation: u64, sequence: u64, user: String, answer: String },
    Error { generation: u64, message: String },
}

struct LlmOpenAiCompatible {
    endpoint: String,
    api_key_env: Option<String>,
    model: String,
    system_prompt: String,
    temperature: f64,
    max_tokens: u32,
    timeout_ms: u64,
    max_results_per_wakeup: usize,
    history_turns: usize,
    sentence_chunk_characters: usize,
    stream: bool,
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    pending: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
    sender: Sender<LlmResult>,
    results: Receiver<LlmResult>,
    history: Vec<(String, String)>,
}

impl LlmOpenAiCompatible {
    fn cancel_current(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        // Detach the previous worker: it observes its own cancel flag, exits on
        // the next SSE read, and its stale results are filtered by generation.
        self.worker = None;
        while self.results.try_recv().is_ok() {
            self.pending.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn start_generation(
        &mut self,
        text: &str,
        sequence: u64,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        self.cancel_current();
        let generation = self.generation;
        let endpoint = format!("{}/chat/completions", self.endpoint);
        let model = self.model.clone();
        let system_prompt = self.system_prompt.clone();
        let api_key_env = self.api_key_env.clone();
        let api_key = api_key_env
            .as_ref()
            .and_then(|name| std::env::var(name).ok());
        let temperature = self.temperature;
        let max_tokens = self.max_tokens;
        let timeout_ms = self.timeout_ms;
        let sentence_chunk_characters = self.sentence_chunk_characters;
        let stream = self.stream;
        let history: Vec<(String, String)> = self
            .history
            .iter()
            .rev()
            .take(self.history_turns)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let user = text.to_owned();
        let mut messages = Vec::with_capacity(2 + history.len() * 2);
        messages.push(serde_json::json!({"role": "system", "content": system_prompt}));
        for (role_user, role_assistant) in &history {
            messages.push(serde_json::json!({"role": "user", "content": role_user}));
            messages.push(serde_json::json!({"role": "assistant", "content": role_assistant}));
        }
        messages.push(serde_json::json!({"role": "user", "content": user}));

        let sender = self.sender.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let pending_count = Arc::clone(&self.pending);
        self.cancel = Some(Arc::clone(&cancel));
        self.worker = Some(thread::Builder::new()
            .name(format!("muxiva-llm-{generation}"))
            .spawn(move || {
                run_llm_request(
                    endpoint,
                    api_key_env,
                    api_key,
                    model,
                    messages,
                    temperature,
                    max_tokens,
                    timeout_ms,
                    sentence_chunk_characters,
                    stream,
                    generation,
                    sequence,
                    user,
                    sender,
                    cancel,
                    pending_count,
                );
            })
            .expect("bounded LLM worker thread"));
        context.schedule_next_tick(Duration::from_millis(20));
        Ok(())
    }

    fn drain(&mut self, context: &mut NodeContext) -> muxiva_types::Result<()> {
        for _ in 0..self.max_results_per_wakeup {
            match self.results.try_recv() {
                Ok(LlmResult::Delta { generation, sequence, text }) if generation == self.generation => {
                    self.pending.fetch_sub(1, Ordering::AcqRel);
                    context.emit(
                        PortName::new("text_out").unwrap(),
                        llm_frame(context.node_id(), sequence, FramePayload::Text(TextData::new(text)), "llm-delta")?,
                    )?;
                }
                Ok(LlmResult::Done { generation, sequence, user, answer }) if generation == self.generation => {
                    self.pending.fetch_sub(1, Ordering::AcqRel);
                    if !answer.is_empty() {
                        self.history.push((user, answer.clone()));
                        let keep = self.history_turns * 2;
                        if self.history.len() > keep {
                            let drain = self.history.len() - keep;
                            self.history.drain(..drain);
                        }
                        context.emit(
                            PortName::new("event_out").unwrap(),
                            llm_frame(
                                context.node_id(),
                                sequence,
                                FramePayload::Event(EventData::new(
                                    NamespacedName::new("muxiva.voice.response.completed")?,
                                    SchemaVersion::new(1)?,
                                    context.node_id().clone(),
                                    Value::String(answer.clone().into()),
                                )),
                                "llm-completed",
                            )?,
                        )?;
                    }
                    break;
                }
                Ok(LlmResult::Error { generation, message }) if generation == self.generation => {
                    self.pending.fetch_sub(1, Ordering::AcqRel);
                    return Err(MuxivaError::new(
                        ErrorCategory::External,
                        "MUXIVA-LLM-REQUEST",
                        message,
                    ));
                }
                Ok(_) => {
                    self.pending.fetch_sub(1, Ordering::AcqRel);
                    continue;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        Ok(())
    }

    fn has_pending_work(&self) -> bool {
        self.worker.as_ref().is_some_and(|worker| !worker.is_finished())
            || self.pending.load(Ordering::Acquire) > 0
    }
}

impl Node for LlmOpenAiCompatible {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        match context.input_port().map(PortName::as_str) {
            Some("text_in") => {
                let input = required_type(input, FrameType::Text, "LLM node requires text")?;
                let text = input.as_text().expect("validated text frame").data().as_str();
                let sequence = input.header().sequence_id().get();
                self.start_generation(text, sequence, context)?;
            }
            _ => {
                self.drain(context)?;
            }
        }
        if self.has_pending_work() {
            context.schedule_next_tick(Duration::from_millis(20));
        }
        Ok(())
    }

    fn on_signal(
        &mut self,
        _signal: muxiva_types::SignalFrame,
        _context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.cancel_current();
        Ok(())
    }

    fn on_finish(&mut self, _context: &mut NodeContext) -> muxiva_types::Result<()> {
        self.cancel_current();
        Ok(())
    }

    fn on_abort(&mut self, _reason: &muxiva_core::AbortReason, _context: &mut NodeContext) {
        self.cancel_current();
    }
}

fn llm_frame(
    node_id: &NodeId,
    sequence: u64,
    payload: FramePayload,
    reason: &'static str,
) -> muxiva_types::Result<Frame> {
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    let frame_type = payload.frame_type();
    Frame::new(
        FrameHeader::new(
            FrameId::new(format!("builtin-{reason}-{serial}")).expect("bounded LLM frame ID"),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("muxiva.builtin.llm").expect("valid LLM clock"),
                ClockKind::Monotonic,
            ),
            SequenceId::new(sequence),
            StreamId::new(format!("llm-{node_id}")).expect("bounded LLM stream"),
            TraceId::new(format!("llm-{node_id}")).expect("bounded LLM trace"),
            frame_type,
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )?,
        payload,
    )
}

fn run_llm_request(
    endpoint: String,
    api_key_env: Option<String>,
    api_key: Option<String>,
    model: String,
    messages: Vec<serde_json::Value>,
    temperature: f64,
    max_tokens: u32,
    timeout_ms: u64,
    sentence_chunk_characters: usize,
    stream: bool,
    generation: u64,
    sequence: u64,
    user: String,
    sender: Sender<LlmResult>,
    cancel: Arc<AtomicBool>,
    pending_count: Arc<AtomicUsize>,
) {
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": max_tokens,
        "stream": stream,
    });
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(timeout_ms)))
            .build(),
    );
    let mut request = agent
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream");
    if let (Some(name), Some(key)) = (api_key_env.as_deref(), api_key.as_deref()) {
        request = request.header("Authorization", &format!("Bearer {key}"));
        eprintln!(
            "[MUXIVA][LLM][request.started] endpoint={endpoint} model={model} auth_env={name}"
        );
    } else {
        eprintln!(
            "[MUXIVA][LLM][request.started] endpoint={endpoint} model={model} auth=none"
        );
    }
    let response = match request.send_json(&body) {
        Ok(response) => response,
        Err(error) => {
            pending_count.fetch_add(1, Ordering::AcqRel);
            let _ = sender.send(LlmResult::Error {
                generation,
                message: format!("LLM HTTP request failed: {error}"),
            });
            return;
        }
    };

    let mut answer = String::new();
    let mut pending = String::new();
    let mut chunks = Vec::new();
    let emit_chunks = |pending: &mut String, chunks: &mut Vec<String>, sender: &Sender<LlmResult>| {
        drain_sentence_chunks(pending, sentence_chunk_characters, chunks);
        for chunk in chunks.drain(..) {
            if !chunk.is_empty() {
                pending_count.fetch_add(1, Ordering::AcqRel);
                let _ = sender.send(LlmResult::Delta {
                    generation,
                    sequence,
                    text: chunk,
                });
            }
        }
    };
    if stream {
        let mut reader = std::io::BufReader::new(response.into_body().into_reader());
        let mut line = String::new();
        loop {
            if cancel.load(Ordering::Acquire) {
                return;
            }
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
            let trimmed = line.trim();
            if let Some(data) = trimmed.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break;
                }
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(delta) = event
                        .get("choices")
                        .and_then(|choices| choices.get(0))
                        .and_then(|choice| choice.get("delta"))
                        .and_then(|delta| delta.get("content"))
                        .and_then(serde_json::Value::as_str)
                    {
                        answer.push_str(delta);
                        pending.push_str(delta);
                        emit_chunks(&mut pending, &mut chunks, &sender);
                    }
                }
            }
        }
    } else {
        let mut body_text = String::new();
        let _ = response.into_body().into_reader().read_to_string(&mut body_text);
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body_text) {
            if let Some(content) = value
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(serde_json::Value::as_str)
            {
                answer.push_str(content);
                pending.push_str(content);
                emit_chunks(&mut pending, &mut chunks, &sender);
            }
        }
    }
    drain_sentence_chunks(&mut pending, sentence_chunk_characters, &mut chunks);
    if !pending.is_empty() {
        chunks.push(std::mem::take(&mut pending));
    }
    for chunk in chunks.drain(..) {
        if !chunk.is_empty() {
            pending_count.fetch_add(1, Ordering::AcqRel);
            let _ = sender.send(LlmResult::Delta {
                generation,
                sequence,
                text: chunk,
            });
        }
    }

    pending_count.fetch_add(1, Ordering::AcqRel);
    let _ = sender.send(LlmResult::Done {
        generation,
        sequence,
        user,
        answer,
    });
}

/// Drains complete spoken sentences (or oversized runs) out of `buffer`.
fn drain_sentence_chunks(buffer: &mut String, max_characters: usize, chunks: &mut Vec<String>) {
    loop {
        let boundary = buffer
            .char_indices()
            .find(|(_, ch)| matches!(ch, '。' | '！' | '？' | '.' | '!' | '?' | '\n'))
            .map(|(index, ch)| index + ch.len_utf8());
        if let Some(end) = boundary {
            chunks.push(buffer.drain(..end).collect());
            continue;
        }
        if buffer.chars().count() >= max_characters {
            let cut = buffer
                .char_indices()
                .nth(max_characters)
                .map(|(index, _)| index)
                .unwrap_or(buffer.len());
            chunks.push(buffer.drain(..cut).collect());
            continue;
        }
        break;
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

fn speech_formatter_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([
                (
                    "code_block_message",
                    map([
                        ("type", Value::String("string".into())),
                        ("minLength", Value::Integer(1)),
                        ("maxLength", Value::Integer(512)),
                        (
                            "default",
                            Value::String("Code is available in the chat.".into()),
                        ),
                    ]),
                ),
                (
                    "table_message",
                    map([
                        ("type", Value::String("string".into())),
                        ("minLength", Value::Integer(1)),
                        ("maxLength", Value::Integer(512)),
                        (
                            "default",
                            Value::String("The detailed table is available in the chat.".into()),
                        ),
                    ]),
                ),
                (
                    "strip_urls",
                    map([
                        ("type", Value::String("boolean".into())),
                        ("default", Value::Bool(true)),
                    ]),
                ),
            ]),
        ),
        (
            "required",
            Value::List(
                vec![
                    Value::String("code_block_message".into()),
                    Value::String("table_message".into()),
                    Value::String("strip_urls".into()),
                ]
                .into_boxed_slice(),
            ),
        ),
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
        println!(
            "[MUXIVA][VOICE][{}] state={} action=observe",
            context.node_id(),
            if active { "speaking" } else { "listening" }
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

fn voice_turn_controller_schema() -> ConfigSchema {
    ConfigSchema::new(map([
        ("type", Value::String("object".into())),
        (
            "properties",
            map([
                (
                    "ignore_filler_utterances",
                    map([
                        ("type", Value::String("boolean".into())),
                        ("default", Value::Bool(true)),
                        (
                            "description",
                            Value::String(
                                "Suppress only exact normalized entries from ignored_utterances; unknown language input fails open".into(),
                            ),
                        ),
                    ]),
                ),
                (
                    "minimum_utterance_characters",
                    map([
                        ("type", Value::String("integer".into())),
                        ("minimum", Value::Integer(1)),
                        ("maximum", Value::Integer(20)),
                        ("default", Value::Integer(1)),
                        (
                            "description",
                            Value::String(
                                "Minimum preview length for early cancellation; never rejects a non-filler final transcript".into(),
                            ),
                        ),
                    ]),
                ),
                (
                    "short_utterance_allowlist",
                    map([
                        ("type", Value::String("array".into())),
                        (
                            "items",
                            map([
                                ("type", Value::String("string".into())),
                                ("minLength", Value::Integer(1)),
                                ("maxLength", Value::Integer(32)),
                            ]),
                        ),
                        ("maxItems", Value::Integer(64)),
                        ("default", Value::List(Vec::new().into_boxed_slice())),
                        (
                            "description",
                            Value::String(
                                "Deployment-owned short commands that may cancel on their first preview".into(),
                            ),
                        ),
                    ]),
                ),
                (
                    "ignored_utterances",
                    map([
                        ("type", Value::String("array".into())),
                        (
                            "items",
                            map([
                                ("type", Value::String("string".into())),
                                ("minLength", Value::Integer(1)),
                                ("maxLength", Value::Integer(32)),
                            ]),
                        ),
                        ("maxItems", Value::Integer(64)),
                        ("default", Value::List(Vec::new().into_boxed_slice())),
                        (
                            "description",
                            Value::String(
                                "Deployment-owned, language-specific exact fillers and non-speech transcripts".into(),
                            ),
                        ),
                    ]),
                ),
            ]),
        ),
        (
            "required",
            Value::List(
                vec![
                    Value::String("ignore_filler_utterances".into()),
                    Value::String("minimum_utterance_characters".into()),
                    Value::String("short_utterance_allowlist".into()),
                    Value::String("ignored_utterances".into()),
                ]
                .into_boxed_slice(),
            ),
        ),
        ("additionalProperties", Value::Bool(false)),
    ]))
}

struct VoiceTurnControllerFactory;

impl NodeFactory for VoiceTurnControllerFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        parse_voice_turn_controller_config(config).map(|_| ())
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let parsed = parse_voice_turn_controller_config(config)?;
        Ok(Box::new(VoiceTurnController {
            ignore_fillers: parsed.ignore_fillers,
            minimum_characters: parsed.minimum_characters,
            allowlist: parsed.allowlist,
            ignored: parsed.ignored,
            generation: 0,
            preview_window_open: false,
            preview_candidate: None,
            preview_hits: 0,
            early_cancel_generation: None,
        }))
    }
}

struct VoiceTurnControllerConfig {
    ignore_fillers: bool,
    minimum_characters: usize,
    allowlist: BTreeSet<String>,
    ignored: BTreeSet<String>,
}

fn parse_voice_turn_controller_config(
    config: &ConfigMap,
) -> Result<VoiceTurnControllerConfig, NodeFactoryError> {
    if config.len() != 4 {
        return Err(config_error(
            "voice turn controller requires exactly four policy fields",
        ));
    }
    let ignore_fillers = match config.get("ignore_filler_utterances") {
        Some(Value::Bool(value)) => *value,
        _ => return Err(config_error("ignore_filler_utterances must be boolean")),
    };
    let minimum_characters = match config.get("minimum_utterance_characters") {
        Some(Value::Integer(value @ 1..=20)) => *value as usize,
        _ => {
            return Err(config_error(
                "minimum_utterance_characters must be between 1 and 20",
            ))
        }
    };
    Ok(VoiceTurnControllerConfig {
        ignore_fillers,
        minimum_characters,
        allowlist: voice_string_set(config, "short_utterance_allowlist")?,
        ignored: voice_string_set(config, "ignored_utterances")?,
    })
}

fn voice_string_set(
    config: &ConfigMap,
    name: &'static str,
) -> Result<BTreeSet<String>, NodeFactoryError> {
    let Some(Value::List(values)) = config.get(name) else {
        return Err(config_error("voice utterance policy lists must be arrays"));
    };
    if values.len() > 64 {
        return Err(config_error(
            "voice utterance policy lists accept at most 64 values",
        ));
    }
    let mut output = BTreeSet::new();
    for value in values {
        let Value::String(value) = value else {
            return Err(config_error("voice utterance policy values must be strings"));
        };
        let normalized = normalize_voice_utterance(value);
        if normalized.is_empty() || normalized.chars().count() > 32 {
            return Err(config_error(
                "voice utterance policy values must contain 1 to 32 characters",
            ));
        }
        output.insert(normalized);
    }
    Ok(output)
}

fn normalize_voice_utterance(value: &str) -> String {
    const PUNCTUATION: &str = "，。！？、,.!?…~～;；:：'\"“”‘’（）()[]【】<>《》*_—-";
    value
        .chars()
        .filter(|character| !character.is_whitespace() && !PUNCTUATION.contains(*character))
        .flat_map(char::to_lowercase)
        .collect()
}

struct VoiceTurnController {
    ignore_fillers: bool,
    minimum_characters: usize,
    allowlist: BTreeSet<String>,
    ignored: BTreeSet<String>,
    generation: u64,
    /// Streaming ASR previews are valid only between speech start and the
    /// corresponding final transcript. Providers may deliver an already
    /// queued preview after the final frame, so this explicit gate prevents a
    /// second cancellation generation from being committed for one utterance.
    preview_window_open: bool,
    preview_candidate: Option<String>,
    preview_hits: u8,
    early_cancel_generation: Option<u64>,
}

impl VoiceTurnController {
    fn can_consume_preview(&self, text: &str) -> bool {
        self.preview_window_open
            && self.early_cancel_generation.is_none()
            && self.preview_rejection_reason(text).is_none()
    }

    fn ignored_reason(&self, text: &str) -> Option<&'static str> {
        let normalized = normalize_voice_utterance(text);
        if normalized.is_empty() {
            return Some("empty");
        }
        // Fail open across languages: Core suppresses only deployment-declared
        // fillers/non-speech. Unknown words, including short English or
        // Spanish utterances, are valid final Turns.
        if self.ignore_fillers && self.ignored.contains(&normalized) {
            return Some("configured");
        }
        None
    }

    fn preview_rejection_reason(&self, text: &str) -> Option<&'static str> {
        if let Some(reason) = self.ignored_reason(text) {
            return Some(reason);
        }
        let normalized = normalize_voice_utterance(text);
        // Length is only an early-interruption confidence gate. It must never
        // discard a final transcript: if no longer preview arrives, the final
        // non-filler utterance is admitted and cancels the previous Turn.
        if normalized.chars().count() < self.minimum_characters
            && !self.allowlist.contains(&normalized)
        {
            return Some("preview_too_short");
        }
        None
    }

    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.generation
    }

    fn decision_payload(
        generation: u64,
        turn_id: u64,
        reason: &str,
    ) -> muxiva_types::Result<Value> {
        Ok(Value::Map(ValueMap::try_from_iter([
            (
                "generation",
                Value::Integer(i64::try_from(generation).unwrap_or(i64::MAX)),
            ),
            (
                "turn_id",
                Value::Integer(i64::try_from(turn_id).unwrap_or(i64::MAX)),
            ),
            ("reason", Value::String(reason.into())),
            (
                "controller",
                Value::String("builtin.voice_turn_controller".into()),
            ),
        ])?))
    }

    fn emit_event(
        input: &Frame,
        context: &mut NodeContext,
        topic: &str,
        payload: Value,
    ) -> muxiva_types::Result<()> {
        let event = derive_payload(
            input,
            context.node_id(),
            "voice-turn-event",
            FramePayload::Event(EventData::new(
                NamespacedName::new(topic)?,
                SchemaVersion::new(1)?,
                context.node_id().clone(),
                payload,
            )),
        )?;
        context.publish_notification(event.as_event().expect("event payload").clone())?;
        context.emit(PortName::new("event_out").unwrap(), event)?;
        Ok(())
    }

    fn commit_cancellation(
        &mut self,
        input: &Frame,
        context: &mut NodeContext,
        reason: &str,
    ) -> muxiva_types::Result<u64> {
        let generation = self.next_generation();
        let turn_id = input.header().sequence_id().get();
        let signal = derive_payload(
            input,
            context.node_id(),
            "voice-turn-cancel",
            FramePayload::Signal(SignalData::new(
                NamespacedName::new(muxiva_types::voice::TURN_CANCELLED)?,
                SchemaVersion::new(1)?,
                context.node_id().clone(),
                Self::decision_payload(generation, turn_id, reason)?,
            )),
        )?;
        context.emit_signal(signal.as_signal().expect("signal payload").clone())?;
        Ok(generation)
    }

    fn clear_preview(&mut self) {
        self.preview_candidate = None;
        self.preview_hits = 0;
    }
}

impl Node for VoiceTurnController {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "MUXIVA-VOICE-TURN-INPUT",
                "voice turn controller requires input",
            )
        })?;
        match context.input_port().map(PortName::as_str) {
            Some("activity_in") => {
                input.ensure_type(FrameType::Event)?;
                let topic = input
                    .as_event()
                    .map(|event| event.data().topic().as_str())
                    .unwrap_or_default();
                if topic == muxiva_types::voice::VOICE_ACTIVITY_STARTED {
                    self.clear_preview();
                    self.early_cancel_generation = None;
                    self.preview_window_open = true;
                }
                context.emit(PortName::new("activity_out").unwrap(), input)?;
            }
            Some("preview_in") => {
                let text = input
                    .as_text()
                    .ok_or_else(|| node_error("MUXIVA-VOICE-TURN-TYPE", "preview must be text"))?
                    .data()
                    .as_str();
                if !self.can_consume_preview(text) {
                    return Ok(());
                }
                let normalized = normalize_voice_utterance(text);
                let compatible = self.preview_candidate.as_ref().is_some_and(|previous| {
                    normalized.starts_with(previous) || previous.starts_with(&normalized)
                });
                if compatible {
                    self.preview_hits = self.preview_hits.saturating_add(1);
                    self.preview_candidate = Some(normalized.clone());
                } else {
                    self.preview_candidate = Some(normalized.clone());
                    self.preview_hits = 1;
                }
                // Two compatible hypotheses fence transient partial mistakes.
                // Explicit short commands are admitted on their first preview.
                if self.preview_hits >= 2 || self.allowlist.contains(&normalized) {
                    let generation = self.commit_cancellation(&input, context, "validated_partial")?;
                    self.early_cancel_generation = Some(generation);
                    println!(
                        "[MUXIVA][VOICE-TURN][early_cancel] turn={} generation={} preview_hits={}",
                        input.header().sequence_id().get(), generation, self.preview_hits
                    );
                }
            }
            Some("transcript_in") => {
                // Close before making the final decision. Any preview already
                // queued behind this frame belongs to the completed utterance.
                self.preview_window_open = false;
                let text = input
                    .as_text()
                    .ok_or_else(|| {
                        node_error("MUXIVA-VOICE-TURN-TYPE", "transcript must be text")
                    })?
                    .data()
                    .as_str()
                    .trim()
                    .to_owned();
                let turn_id = input.header().sequence_id().get();
                if let Some(reason) = self.ignored_reason(&text) {
                    self.clear_preview();
                    self.early_cancel_generation = None;
                    Self::emit_event(
                        &input,
                        context,
                        muxiva_types::voice::TURN_UTTERANCE_IGNORED,
                        Self::decision_payload(self.generation, turn_id, reason)?,
                    )?;
                    println!(
                        "[MUXIVA][VOICE-TURN][ignored] turn={} generation={} reason={}",
                        turn_id, self.generation, reason
                    );
                    return Ok(());
                }

                let generation = if let Some(generation) = self.early_cancel_generation.take() {
                    generation
                } else {
                    self.commit_cancellation(&input, context, "new_turn")?
                };
                self.clear_preview();
                let prompt = derive_payload(
                    &input,
                    context.node_id(),
                    "voice-turn-prompt",
                    FramePayload::Text(TextData::new(text.into_boxed_str())),
                )?;
                context.emit(PortName::new("prompt_out").unwrap(), prompt.clone())?;
                context.emit(PortName::new("transcript_out").unwrap(), prompt)?;
                for topic in [
                    muxiva_types::voice::TURN_STARTED,
                    muxiva_types::voice::TURN_UTTERANCE_COMMITTED,
                ] {
                    Self::emit_event(
                        &input,
                        context,
                        topic,
                        Self::decision_payload(generation, turn_id, "admitted")?,
                    )?;
                }
                println!(
                    "[MUXIVA][VOICE-TURN][committed] turn={} generation={} action=cancel_previous_and_start",
                    turn_id, generation
                );
            }
            _ => {
                return Err(node_error(
                    "MUXIVA-VOICE-TURN-PORT",
                    "voice turn controller received an unknown data port",
                ))
            }
        }
        Ok(())
    }

    fn on_signal(
        &mut self,
        signal: muxiva_types::SignalFrame,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = Frame::Signal(signal);
        let generation = self.commit_cancellation(&input, context, "authoritative_interrupt")?;
        Self::emit_event(
            &input,
            context,
            muxiva_types::voice::TURN_CANCELLED,
            Self::decision_payload(
                generation,
                input.header().sequence_id().get(),
                "authoritative_interrupt",
            )?,
        )?;
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

struct SpeechFormatterFactory;

impl NodeFactory for SpeechFormatterFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        let valid_message = |name: &str| matches!(config.get(name), Some(Value::String(value)) if !value.trim().is_empty() && value.len() <= 512);
        if config.len() == 3
            && valid_message("code_block_message")
            && valid_message("table_message")
            && matches!(config.get("strip_urls"), Some(Value::Bool(_)))
        {
            Ok(())
        } else {
            Err(config_error(
                "speech formatter requires bounded code_block_message, table_message, and strip_urls values",
            ))
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let message = |name: &str| match config.get(name) {
            Some(Value::String(value)) => value.clone(),
            _ => unreachable!("validated speech formatter configuration"),
        };
        let strip_urls = match config.get("strip_urls") {
            Some(Value::Bool(value)) => *value,
            _ => unreachable!("validated speech formatter configuration"),
        };
        Ok(Box::new(SpeechFormatter {
            code_block_message: message("code_block_message"),
            table_message: message("table_message"),
            strip_urls,
            in_fenced_code: false,
            in_table: false,
            pending_backticks: 0,
            in_bare_url: false,
            active_sequence: None,
        }))
    }
}

/// Converts display-oriented Markdown into short, deterministic TTS input.
///
/// This Node deliberately sits between an Agent and a TTS Node. The original
/// Agent Text can still fan out to a rich client while only the derived plain
/// Text reaches speech synthesis. It is stateful because fenced code markers
/// and tables can span streaming Text Frames.
struct SpeechFormatter {
    code_block_message: Box<str>,
    table_message: Box<str>,
    strip_urls: bool,
    in_fenced_code: bool,
    in_table: bool,
    pending_backticks: usize,
    in_bare_url: bool,
    active_sequence: Option<u64>,
}

impl SpeechFormatter {
    fn begin_sequence(&mut self, sequence: u64) {
        if self.active_sequence == Some(sequence) {
            return;
        }
        self.active_sequence = Some(sequence);
        self.in_fenced_code = false;
        self.in_table = false;
        self.pending_backticks = 0;
        self.in_bare_url = false;
    }

    fn format_chunk(&mut self, input: &str) -> String {
        let mut combined = "`".repeat(self.pending_backticks);
        combined.push_str(input);
        self.pending_backticks = 0;

        let trailing_backticks = combined
            .as_bytes()
            .iter()
            .rev()
            .take_while(|byte| **byte == b'`')
            .count();
        let retained = if trailing_backticks < 3 {
            trailing_backticks
        } else {
            0
        };
        if retained > 0 {
            combined.truncate(combined.len() - retained);
            self.pending_backticks = retained;
        }

        let mut output = String::new();
        let mut cursor = 0;
        while cursor < combined.len() {
            let Some(relative) = combined[cursor..].find('`') else {
                if !self.in_fenced_code {
                    self.append_markdown_text(&combined[cursor..], &mut output);
                }
                break;
            };
            let marker = cursor + relative;
            if !self.in_fenced_code {
                self.append_markdown_text(&combined[cursor..marker], &mut output);
            }
            let run = combined.as_bytes()[marker..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if run >= 3 {
                self.in_fenced_code = !self.in_fenced_code;
                self.in_table = false;
                if self.in_fenced_code {
                    append_phrase(&mut output, &self.code_block_message);
                }
            }
            // One or two backticks are inline-code formatting. Their content
            // remains speakable, while the formatting markers are discarded.
            cursor = marker + run;
        }
        collapse_spoken_whitespace(&output)
    }

    fn append_markdown_text(&mut self, input: &str, output: &mut String) {
        for line in input.split_inclusive('\n') {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                self.in_table = false;
                continue;
            }
            if looks_like_markdown_table(trimmed) {
                if !self.in_table {
                    append_phrase(output, &self.table_message);
                }
                self.in_table = true;
                continue;
            }
            self.in_table = false;
            let without_prefix = strip_markdown_prefix(trimmed);
            let linked = replace_markdown_links(without_prefix);
            let without_urls = if self.strip_urls {
                self.remove_bare_urls(&linked)
            } else {
                linked
            };
            let plain = without_urls
                .chars()
                .filter(|character| {
                    !matches!(
                        character,
                        '*' | '_' | '~' | '`' | '[' | ']' | '(' | ')' | '{' | '}'
                    )
                })
                .collect::<String>();
            append_phrase(output, plain.trim());
        }
    }

    fn remove_bare_urls(&mut self, input: &str) -> String {
        let mut output = String::new();
        let mut rest = input;
        loop {
            if self.in_bare_url {
                let end = rest
                    .char_indices()
                    .find_map(|(index, character)| is_url_delimiter(character).then_some(index));
                let Some(end) = end else {
                    return output;
                };
                self.in_bare_url = false;
                rest = &rest[end..];
            }
            let start = ["https://", "http://", "www."]
                .iter()
                .filter_map(|prefix| rest.find(prefix))
                .min();
            let Some(start) = start else {
                output.push_str(rest);
                return output;
            };
            output.push_str(&rest[..start]);
            let url = &rest[start..];
            let end = url.char_indices().find_map(|(index, character)| {
                (index > 0 && is_url_delimiter(character)).then_some(index)
            });
            let Some(end) = end else {
                self.in_bare_url = true;
                return output;
            };
            rest = &url[end..];
        }
    }
}

impl Node for SpeechFormatter {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let input = required_type(input, FrameType::Text, "speech formatter requires text")?;
        self.begin_sequence(input.header().sequence_id().get());
        let source = input
            .as_text()
            .expect("validated text frame")
            .data()
            .as_str();
        let spoken = self.format_chunk(source);
        if !spoken.is_empty() {
            context.emit(
                PortName::new(TEXT_OUTPUT).expect("valid built-in port"),
                derive_payload(
                    &input,
                    context.node_id(),
                    "speech-formatter",
                    FramePayload::Text(TextData::new(spoken)),
                )?,
            )?;
        }
        Ok(())
    }

    fn on_signal(
        &mut self,
        _signal: muxiva_types::SignalFrame,
        _context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.active_sequence = None;
        self.in_fenced_code = false;
        self.in_table = false;
        self.pending_backticks = 0;
        self.in_bare_url = false;
        Ok(())
    }
}

fn append_phrase(output: &mut String, phrase: &str) {
    if phrase.is_empty() {
        return;
    }
    if !output.is_empty() && !output.chars().last().is_some_and(char::is_whitespace) {
        output.push(' ');
    }
    output.push_str(phrase);
}

fn strip_markdown_prefix(mut line: &str) -> &str {
    line = line.trim_start_matches(|character: char| character.is_whitespace());
    line = line.trim_start_matches('#').trim_start();
    line = line.trim_start_matches('>').trim_start();
    if let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("+ "))
        .or_else(|| line.strip_prefix("* "))
    {
        return rest;
    }
    let digits = line
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits > 0
        && line
            .get(digits..)
            .is_some_and(|rest| rest.starts_with(". ") || rest.starts_with(") "))
    {
        return &line[digits + 2..];
    }
    line
}

fn looks_like_markdown_table(line: &str) -> bool {
    let pipes = line.bytes().filter(|byte| *byte == b'|').count();
    pipes >= 2
        || (pipes >= 1
            && line
                .chars()
                .all(|character| matches!(character, '|' | '-' | ':' | ' ' | '\t')))
}

fn replace_markdown_links(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;
    while let Some(open) = rest.find('[') {
        let Some(close_relative) = rest[open + 1..].find(']') else {
            break;
        };
        let close = open + 1 + close_relative;
        let after_label = &rest[close + 1..];
        if !after_label.starts_with('(') {
            output.push_str(&rest[..=close]);
            rest = after_label;
            continue;
        }
        let Some(target_end) = after_label[1..].find(')') else {
            break;
        };
        let before = &rest[..open];
        output.push_str(before.strip_suffix('!').unwrap_or(before));
        output.push_str(&rest[open + 1..close]);
        rest = &after_label[target_end + 2..];
    }
    output.push_str(rest);
    output
}

fn is_url_delimiter(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            ',' | '!' | '?' | ';' | ')' | ']' | '}' | '，' | '。' | '！' | '？' | '；' | '、'
        )
}

fn collapse_spoken_whitespace(input: &str) -> String {
    let mut output = String::new();
    let mut whitespace = false;
    for character in input.chars() {
        if character.is_whitespace() {
            whitespace = !output.is_empty();
        } else {
            if whitespace
                && !matches!(
                    character,
                    ',' | '.' | '!' | '?' | ':' | ';' | '，' | '。' | '！' | '？' | '：' | '；'
                )
            {
                output.push(' ');
            }
            whitespace = false;
            output.push(character);
        }
    }
    output.trim().to_owned()
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

    fn voice_turn_controller() -> VoiceTurnController {
        VoiceTurnController {
            ignore_fillers: true,
            minimum_characters: 3,
            allowlist: ["闭嘴".to_owned(), "天气".to_owned()]
                .into_iter()
                .collect(),
            ignored: ["嗯", "额", "咳嗽声", "um", "eh"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            generation: 0,
            preview_window_open: false,
            preview_candidate: None,
            preview_hits: 0,
            early_cancel_generation: None,
        }
    }

    #[test]
    fn voice_activity_is_observational_and_never_emits_cancellation() {
        let mut vad = AudioVad {
            threshold: 1,
            start_frames: 1,
            stop_frames: 1,
            loud_frames: 0,
            quiet_frames: 0,
            active: false,
        };
        let node_id = NodeId::new("vad-test").unwrap();
        let mut context = NodeContext::new(node_id, ConfigMap::empty(), None);
        vad.transition(true, &source_frame("activity").unwrap(), &mut context)
            .unwrap();
        assert!(context.signals().is_empty());
        assert_eq!(context.emissions().len(), 1);
        assert_eq!(
            context.emissions()[0]
                .frame()
                .as_event()
                .unwrap()
                .data()
                .topic()
                .as_str(),
            muxiva_types::voice::VOICE_ACTIVITY_STARTED
        );
    }

    #[test]
    fn voice_turn_policy_rejects_fillers_coughs_and_tiny_echoes() {
        let controller = voice_turn_controller();
        assert_eq!(controller.ignored_reason("嗯……"), Some("configured"));
        assert_eq!(controller.ignored_reason("额。"), Some("configured"));
        assert_eq!(controller.ignored_reason("（咳嗽声）"), Some("configured"));
        assert_eq!(controller.ignored_reason("um"), Some("configured"));
        assert_eq!(controller.ignored_reason("EH"), Some("configured"));
        assert_eq!(controller.ignored_reason("好"), None);
        assert_eq!(controller.ignored_reason("go"), None);
        assert_eq!(controller.ignored_reason("sí"), None);
        assert_eq!(controller.ignored_reason("闭嘴"), None);
        assert_eq!(controller.ignored_reason("榴莲为什么这么臭？"), None);
    }

    #[test]
    fn voice_turn_rejects_late_preview_after_final_window_closes() {
        let mut controller = voice_turn_controller();
        controller.preview_window_open = true;
        assert!(!controller.can_consume_preview("额"));
        assert!(!controller.can_consume_preview("um"));
        assert!(!controller.can_consume_preview("go"));
        assert!(!controller.can_consume_preview("sí"));
        assert!(controller.can_consume_preview("请继续介绍"));
        assert!(controller.can_consume_preview("please continue"));
        assert!(controller.can_consume_preview("continúa por favor"));

        // Final transcript processing closes this gate before it commits the
        // turn, so a queued provider preview cannot cancel the new generation.
        controller.preview_window_open = false;
        assert!(!controller.can_consume_preview("请继续介绍机器人"));
    }

    #[test]
    fn voice_turn_generation_and_payload_are_monotonic_and_explicit() {
        let mut controller = voice_turn_controller();
        assert_eq!(controller.next_generation(), 1);
        assert_eq!(controller.next_generation(), 2);
        let Value::Map(payload) =
            VoiceTurnController::decision_payload(2, 91, "admitted").unwrap()
        else {
            panic!("decision payload must be a map");
        };
        assert_eq!(payload.get("generation"), Some(&Value::Integer(2)));
        assert_eq!(payload.get("turn_id"), Some(&Value::Integer(91)));
        assert_eq!(
            muxiva_types::voice::TURN_CANCELLED,
            "muxiva.turn.cancelled"
        );
    }

    fn speech_formatter() -> SpeechFormatter {
        SpeechFormatter {
            code_block_message: "代码已经生成，请在聊天窗口查看。".into(),
            table_message: "详细表格请在聊天窗口查看。".into(),
            strip_urls: true,
            in_fenced_code: false,
            in_table: false,
            pending_backticks: 0,
            in_bare_url: false,
            active_sequence: None,
        }
    }

    #[test]
    fn speech_formatter_preserves_meaning_without_reading_markdown_or_urls() {
        let mut formatter = speech_formatter();
        assert_eq!(
            formatter.format_chunk(
                "## **结果**\n- 查看[使用文档](https://example.com/guide)，或访问 https://example.com。"
            ),
            "结果 查看使用文档，或访问。"
        );
    }

    #[test]
    fn speech_formatter_handles_streamed_code_fences_and_tables() {
        let mut formatter = speech_formatter();
        assert_eq!(formatter.format_chunk("下面是实现：\n`"), "下面是实现：");
        assert_eq!(
            formatter.format_chunk("``typescript\nconst answer = 42;\n"),
            "代码已经生成，请在聊天窗口查看。"
        );
        assert_eq!(formatter.format_chunk("``"), "");
        assert_eq!(formatter.format_chunk("`\n运行完成。"), "运行完成。");
        assert_eq!(
            formatter.format_chunk("| 名称 | 结果 |\n"),
            "详细表格请在聊天窗口查看。"
        );
        assert_eq!(formatter.format_chunk("| --- | --- |\n"), "");
        assert_eq!(formatter.format_chunk("结论正常。"), "结论正常。");
    }

    #[test]
    fn speech_formatter_suppresses_urls_split_across_agent_chunks() {
        let mut formatter = speech_formatter();
        assert_eq!(
            formatter.format_chunk("查看[使用文档](https://example."),
            "查看使用文档"
        );
        assert_eq!(
            formatter.format_chunk("com/guide)，然后继续。"),
            "，然后继续。"
        );
    }

    #[test]
    fn speech_formatter_resets_incomplete_markdown_at_a_new_turn() {
        let mut formatter = speech_formatter();
        formatter.begin_sequence(10);
        assert_eq!(
            formatter.format_chunk("```rust\nlet stale = true;"),
            "代码已经生成，请在聊天窗口查看。"
        );
        formatter.begin_sequence(11);
        assert_eq!(
            formatter.format_chunk("下一轮仍然可以正常播报。"),
            "下一轮仍然可以正常播报。"
        );
    }

    #[test]
    fn llm_sentence_chunks_split_on_boundaries_and_overflow() {
        let mut buffer = String::from("第一句。第二句！第三句？");
        let mut chunks = Vec::new();
        drain_sentence_chunks(&mut buffer, 80, &mut chunks);
        assert_eq!(chunks, vec!["第一句。", "第二句！", "第三句？"]);
        assert!(buffer.is_empty());

        let mut buffer = String::from("没有标点符号的超长回答");
        let mut chunks = Vec::new();
        drain_sentence_chunks(&mut buffer, 4, &mut chunks);
        assert_eq!(chunks, vec!["没有标点", "符号的超"]);
        assert_eq!(buffer, "长回答");
    }

    #[test]
    fn llm_config_requires_endpoint_and_model() {
        let factory = LlmOpenAiCompatibleFactory;
        let valid = || {
            ConfigMap::try_from_iter(vec![
                (
                    muxiva_core::ConfigKey::new("endpoint").unwrap(),
                    Value::String("https://api.deepseek.com/v1".into()),
                ),
                (
                    muxiva_core::ConfigKey::new("api_key_env").unwrap(),
                    Value::String("DEEPSEEK_API_KEY".into()),
                ),
                (
                    muxiva_core::ConfigKey::new("model").unwrap(),
                    Value::String("deepseek-chat".into()),
                ),
                (
                    muxiva_core::ConfigKey::new("system_prompt").unwrap(),
                    Value::String(LLM_DEFAULT_SYSTEM_PROMPT.into()),
                ),
                (
                    muxiva_core::ConfigKey::new("temperature").unwrap(),
                    Value::Float(muxiva_types::FiniteF64::new(0.6).unwrap()),
                ),
                (
                    muxiva_core::ConfigKey::new("max_tokens").unwrap(),
                    Value::Integer(512),
                ),
                (
                    muxiva_core::ConfigKey::new("timeout_ms").unwrap(),
                    Value::Integer(60_000),
                ),
                (
                    muxiva_core::ConfigKey::new("max_results_per_wakeup").unwrap(),
                    Value::Integer(32),
                ),
                (
                    muxiva_core::ConfigKey::new("history_turns").unwrap(),
                    Value::Integer(6),
                ),
                (
                    muxiva_core::ConfigKey::new("sentence_chunk_characters").unwrap(),
                    Value::Integer(80),
                ),
                (
                    muxiva_core::ConfigKey::new("stream").unwrap(),
                    Value::Bool(true),
                ),
            ])
            .unwrap()
        };
        assert!(factory.validate_config(&valid()).is_ok());

        let missing_model = ConfigMap::try_from_iter(
            valid()
                .iter()
                .filter(|(key, _)| key.as_str() != "model")
                .map(|(key, value)| (key.clone(), value.clone())),
        )
        .unwrap();
        assert!(factory.validate_config(&missing_model).is_err());
    }

    #[test]
    fn llm_openai_compatible_streams_sse_into_text_and_event_frames() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        // A minimal OpenAI-compatible streaming endpoint for the test.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"，世界\"}}]}\n\n",
            "data: [DONE]\n\n",
        );
        let responder = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });

        let config = ConfigMap::try_from_iter(vec![
            (
                muxiva_core::ConfigKey::new("endpoint").unwrap(),
                Value::String(format!("http://127.0.0.1:{port}/v1").into()),
            ),
            (
                muxiva_core::ConfigKey::new("api_key_env").unwrap(),
                Value::String(String::new().into()),
            ),
            (
                muxiva_core::ConfigKey::new("model").unwrap(),
                Value::String("test-model".into()),
            ),
            (
                muxiva_core::ConfigKey::new("system_prompt").unwrap(),
                Value::String("assistant".into()),
            ),
            (
                muxiva_core::ConfigKey::new("temperature").unwrap(),
                Value::Float(muxiva_types::FiniteF64::new(0.0).unwrap()),
            ),
            (
                muxiva_core::ConfigKey::new("max_tokens").unwrap(),
                Value::Integer(32),
            ),
            (
                muxiva_core::ConfigKey::new("timeout_ms").unwrap(),
                Value::Integer(5_000),
            ),
            (
                muxiva_core::ConfigKey::new("max_results_per_wakeup").unwrap(),
                Value::Integer(32),
            ),
            (
                muxiva_core::ConfigKey::new("history_turns").unwrap(),
                Value::Integer(0),
            ),
            (
                muxiva_core::ConfigKey::new("sentence_chunk_characters").unwrap(),
                Value::Integer(80),
            ),
            (
                muxiva_core::ConfigKey::new("stream").unwrap(),
                Value::Bool(true),
            ),
        ])
        .unwrap();
        let node_id = NodeId::new("llm-test").unwrap();
        let mut node = LlmOpenAiCompatibleFactory
            .create(&node_id, &config)
            .unwrap();

        let mut text_input = NodeContext::new(
            node_id.clone(),
            config.clone(),
            Some(PortName::new("text_in").unwrap()),
        );
        node.on_process(Some(source_frame("测试").unwrap()), &mut text_input)
            .unwrap();

        let mut received_text = String::new();
        let mut saw_completed = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let mut tick = NodeContext::new(node_id.clone(), config.clone(), None);
            node.on_process(None, &mut tick).unwrap();
            for emission in tick.take_emissions() {
                match emission.output_port().as_str() {
                    "text_out" => received_text.push_str(
                        emission
                            .frame()
                            .as_text()
                            .unwrap()
                            .data()
                            .as_str(),
                    ),
                    "event_out" => {
                        let event = emission.frame().as_event().unwrap();
                        if event.data().topic().as_str() == "muxiva.voice.response.completed" {
                            saw_completed = true;
                        }
                    }
                    _ => {}
                }
            }
            if saw_completed {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        responder.join().unwrap();
        assert_eq!(received_text, "你好，世界");
        assert!(saw_completed);
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
