use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use voxa_core::{
    ConfigMap, ConfigSchema, LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeFactory,
    NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration, NodeRegistry,
    NodeTypeName, PortDescriptor, PortDirection, PortName, ResourceKey,
};
use voxa_types::{
    AudioData, AudioLayout, ErrorCategory, Frame, FrameBuffer, FrameDerivation, FrameId,
    FramePayload, FrameType, NodeId, PcmSampleFormat, TextData, TransformOrigin, Value, VoxaError,
};

use crate::{
    QwenCredentials, QwenRealtimeConfig, QwenRealtimeSocket, QwenServerEvent, QwenTurnDetection,
    QWEN_CREDENTIALS_RESOURCE,
};

pub const QWEN_AUDIO_REALTIME: &str = "provider.qwen.audio_realtime";
pub const QWEN_FACTORY_VERSION: &str = "1.0.0";
const AUDIO_INPUT: &str = "audio_in";
const AUDIO_OUTPUT: &str = "audio_out";
const TEXT_OUTPUT: &str = "text_out";
const MAX_EVENTS_PER_CALL: usize = 64;
static NEXT_FRAME: AtomicU64 = AtomicU64::new(1);

pub fn register_qwen_nodes(registry: &mut NodeRegistry) -> Result<(), voxa_core::RegistryError> {
    let template_id = NodeId::new("template-provider-qwen-audio-realtime").expect("valid ID");
    let descriptor = NodeDescriptor::new(
        template_id.clone(),
        NodeTypeName::new(QWEN_AUDIO_REALTIME).expect("valid node type"),
        NodeKind::Transform,
        [
            (AUDIO_INPUT, PortDirection::Input, FrameType::Audio),
            (AUDIO_OUTPUT, PortDirection::Output, FrameType::Audio),
            (TEXT_OUTPUT, PortDirection::Output, FrameType::Text),
        ]
        .into_iter()
        .map(|(name, direction, frame_type)| {
            PortDescriptor::new(
                template_id.clone(),
                PortName::new(name).expect("valid port"),
                direction,
                frame_type,
            )
        })
        .collect::<Vec<_>>(),
        ConfigSchema::empty(),
        LifecycleCapabilities::new(true, true, true, true),
    );
    registry.register(NodeRegistration::new(
        NodeLanguage::Rust,
        descriptor,
        NodeFactoryVersion::new(QWEN_FACTORY_VERSION).expect("valid version"),
        Arc::new(QwenAudioRealtimeFactory),
    ))
}

struct QwenAudioRealtimeFactory;

impl NodeFactory for QwenAudioRealtimeFactory {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        for (key, value) in config.iter() {
            if !matches!(
                key.as_str(),
                "model" | "voice" | "instructions" | "turn_detection"
            ) {
                return Err(factory_error(
                    "VOXA-QWEN-CONFIG-KEY",
                    "unsupported Qwen configuration field",
                ));
            }
            let Value::String(value) = value else {
                return Err(factory_error(
                    "VOXA-QWEN-CONFIG-TYPE",
                    "Qwen configuration values must be strings",
                ));
            };
            if value.is_empty() || value.len() > 16 * 1024 {
                return Err(factory_error(
                    "VOXA-QWEN-CONFIG-LENGTH",
                    "invalid Qwen configuration value length",
                ));
            }
        }
        if let Some(Value::String(value)) = config.get("turn_detection") {
            if !matches!(value.as_ref(), "server_vad" | "smart_turn") {
                return Err(factory_error(
                    "VOXA-QWEN-TURN-DETECTION",
                    "turn_detection must be server_vad or smart_turn",
                ));
            }
        }
        Ok(())
    }

    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        let mut realtime = QwenRealtimeConfig::default();
        if let Some(Value::String(value)) = config.get("model") {
            realtime.model = value.to_string();
        }
        if let Some(Value::String(value)) = config.get("voice") {
            realtime.voice = value.to_string();
        }
        if let Some(Value::String(value)) = config.get("instructions") {
            realtime.instructions = value.to_string();
        }
        if let Some(Value::String(value)) = config.get("turn_detection") {
            realtime.turn_detection = if value.as_ref() == "server_vad" {
                QwenTurnDetection::ServerVad
            } else {
                QwenTurnDetection::SmartTurn
            };
        }
        Ok(Box::new(QwenAudioRealtimeNode {
            config: realtime,
            socket: None,
        }))
    }
}

struct QwenAudioRealtimeNode {
    config: QwenRealtimeConfig,
    socket: Option<QwenRealtimeSocket>,
}

impl Node for QwenAudioRealtimeNode {
    fn on_prepare(&mut self, context: &mut NodeContext) -> voxa_types::Result<()> {
        let key = ResourceKey::new(QWEN_CREDENTIALS_RESOURCE).map_err(resource_error)?;
        let credentials = context
            .resources()
            .get::<QwenCredentials>(&key)
            .map_err(resource_error)?;
        self.socket = Some(
            QwenRealtimeSocket::connect(&credentials, &self.config)
                .map_err(|error| provider_error("VOXA-QWEN-CONNECT", error))?,
        );
        Ok(())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            node_error(
                "VOXA-QWEN-INPUT-MISSING",
                "Qwen realtime requires audio input",
            )
        })?;
        let audio = input.as_audio().ok_or_else(|| {
            node_error("VOXA-QWEN-INPUT-TYPE", "Qwen realtime requires audio input")
        })?;
        validate_input_audio(audio.data())?;
        let socket = self.socket.as_mut().ok_or_else(|| {
            node_error(
                "VOXA-QWEN-NOT-PREPARED",
                "Qwen realtime connection is not prepared",
            )
        })?;
        socket
            .send_audio(audio.data().buffer().as_slice())
            .map_err(|error| provider_error("VOXA-QWEN-SEND", error))?;
        for _ in 0..MAX_EVENTS_PER_CALL {
            let Some(event) = socket
                .try_read_event()
                .map_err(|error| provider_error("VOXA-QWEN-RECEIVE", error))?
            else {
                break;
            };
            emit_event(event, &input, context)?;
        }
        Ok(())
    }

    fn on_signal(
        &mut self,
        signal: voxa_types::SignalFrame,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        if signal.data().name().as_str() == "voxa.runtime.interrupt" {
            self.socket
                .as_mut()
                .ok_or_else(|| {
                    node_error(
                        "VOXA-QWEN-NOT-PREPARED",
                        "Qwen realtime connection is not prepared",
                    )
                })?
                .cancel_response()
                .map_err(|error| provider_error("VOXA-QWEN-CANCEL", error))?;
        }
        Ok(())
    }

    fn on_finish(&mut self, _context: &mut NodeContext) -> voxa_types::Result<()> {
        self.socket.take();
        Ok(())
    }

    fn on_abort(&mut self, _reason: &voxa_core::AbortReason, _context: &mut NodeContext) {
        self.socket.take();
    }
}

fn validate_input_audio(audio: &AudioData) -> voxa_types::Result<()> {
    if audio.sample_rate_hz() != 16_000
        || audio.channels() != 1
        || audio.sample_format() != PcmSampleFormat::I16Le
        || audio.layout() != AudioLayout::Interleaved
    {
        return Err(node_error(
            "VOXA-QWEN-AUDIO-FORMAT",
            "Qwen Audio Realtime input must be mono interleaved PCM s16le at 16000 Hz",
        ));
    }
    Ok(())
}

fn emit_event(
    event: QwenServerEvent,
    parent: &Frame,
    context: &mut NodeContext,
) -> voxa_types::Result<()> {
    match event {
        QwenServerEvent::ResponseAudio(bytes) if !bytes.is_empty() => {
            let samples = u64::try_from(bytes.len() / 2)
                .map_err(|_| node_error("VOXA-QWEN-AUDIO-SIZE", "Qwen audio size overflow"))?;
            let audio = AudioData::new(
                FrameBuffer::from_vec(bytes),
                24_000,
                1,
                PcmSampleFormat::I16Le,
                AudioLayout::Interleaved,
                samples,
            )?;
            context.emit(
                PortName::new(AUDIO_OUTPUT).unwrap(),
                derive(
                    parent,
                    context.node_id(),
                    FramePayload::Audio(audio),
                    "qwen_realtime_audio",
                )?,
            )?;
        }
        QwenServerEvent::ResponseTranscriptDelta(text)
        | QwenServerEvent::InputTranscriptDelta(text)
            if !text.is_empty() =>
        {
            context.emit(
                PortName::new(TEXT_OUTPUT).unwrap(),
                derive(
                    parent,
                    context.node_id(),
                    FramePayload::Text(TextData::new(text)),
                    "qwen_realtime_transcript",
                )?,
            )?;
        }
        QwenServerEvent::Error { code, message } => {
            return Err(VoxaError::new(
                ErrorCategory::External,
                "VOXA-QWEN-PROVIDER",
                "Qwen provider returned an error",
            )
            .with_context("provider_code", code)
            .with_context("provider_message", message));
        }
        _ => {}
    }
    Ok(())
}

fn derive(
    parent: &Frame,
    node_id: &NodeId,
    payload: FramePayload,
    reason: &str,
) -> voxa_types::Result<Frame> {
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    parent.derive(
        FrameDerivation::new(
            FrameId::new(format!("qwen-realtime-{serial}")).expect("bounded ID"),
            parent.header().timestamp(),
            parent.header().sequence_id(),
            TransformOrigin::new(Some(node_id.clone()), None)?,
            reason,
        )?
        .with_payload(payload),
    )
}

fn factory_error(code: &'static str, message: &'static str) -> NodeFactoryError {
    NodeFactoryError::new(code, message)
}

fn node_error(code: &'static str, message: &'static str) -> VoxaError {
    VoxaError::new(ErrorCategory::Validation, code, message)
}

fn provider_error(code: &'static str, error: impl std::fmt::Display) -> VoxaError {
    VoxaError::new(ErrorCategory::External, code, error.to_string())
}

fn resource_error(error: impl std::fmt::Display) -> VoxaError {
    VoxaError::new(
        ErrorCategory::Validation,
        "VOXA-QWEN-CREDENTIALS",
        error.to_string(),
    )
}
