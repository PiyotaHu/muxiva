#![forbid(unsafe_code)]
//! Qwen multimodal provider adapters for Voxa.
//!
//! Credentials are runtime resources, never serializable node configuration.

mod node;
mod protocol;
mod realtime;

pub use node::{register_qwen_nodes, QWEN_AUDIO_REALTIME, QWEN_FACTORY_VERSION};
pub use protocol::{
    append_audio_message, cancel_response_message, parse_server_event, session_update_message,
    QwenServerEvent, QwenTurnDetection,
};
pub use realtime::{
    QwenCredentials, QwenRealtimeConfig, QwenRealtimeError, QwenRealtimeSocket,
    QWEN_CREDENTIALS_RESOURCE,
};
