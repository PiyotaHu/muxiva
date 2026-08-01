use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};

const MAX_SERVER_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_AUDIO_CHUNK_BYTES: usize = 256 * 1024;

/// Server-side turn boundary strategy supported by Qwen Audio Realtime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QwenTurnDetection {
    ServerVad,
    SmartTurn,
}

impl QwenTurnDetection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ServerVad => "server_vad",
            Self::SmartTurn => "smart_turn",
        }
    }
}

/// Bounded, provider-neutral observations decoded from Qwen server events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QwenServerEvent {
    SessionCreated,
    SessionUpdated,
    SpeechStarted,
    SpeechStopped,
    InputTranscriptDelta(String),
    InputTranscriptDone(String),
    ResponseAudio(Vec<u8>),
    ResponseTranscriptDelta(String),
    ResponseTranscriptDone(String),
    ResponseDone { cancelled: bool },
    Error { code: String, message: String },
    Ignored { event_type: String },
}

pub fn session_update_message(
    voice: &str,
    instructions: &str,
    turn_detection: QwenTurnDetection,
) -> Value {
    json!({
        "event_id": "voxa-session-update",
        "type": "session.update",
        "session": {
            "modalities": ["text", "audio"],
            "voice": voice,
            "instructions": instructions,
            "input_audio_format": "pcm16",
            "output_audio_format": "pcm16",
            "input_audio_transcription": { "model": "gummy-realtime-v1" },
            "turn_detection": { "type": turn_detection.as_str() }
        }
    })
}

pub fn append_audio_message(audio: &[u8]) -> Result<Value, &'static str> {
    if audio.is_empty() {
        return Err("audio chunk must not be empty");
    }
    if audio.len() > MAX_AUDIO_CHUNK_BYTES {
        return Err("audio chunk exceeds 256 KiB");
    }
    Ok(json!({
        "event_id": "voxa-audio-append",
        "type": "input_audio_buffer.append",
        "audio": STANDARD.encode(audio)
    }))
}

pub fn cancel_response_message() -> Value {
    json!({
        "event_id": "voxa-response-cancel",
        "type": "response.cancel"
    })
}

pub fn parse_server_event(input: &str) -> Result<QwenServerEvent, &'static str> {
    if input.len() > MAX_SERVER_MESSAGE_BYTES {
        return Err("Qwen server message exceeds 8 MiB");
    }
    let value: Value = serde_json::from_str(input).map_err(|_| "invalid Qwen server JSON")?;
    let event_type = required_string(&value, "type")?;
    let event = match event_type {
        "session.created" => QwenServerEvent::SessionCreated,
        "session.updated" => QwenServerEvent::SessionUpdated,
        "input_audio_buffer.speech_started" => QwenServerEvent::SpeechStarted,
        "input_audio_buffer.speech_stopped" => QwenServerEvent::SpeechStopped,
        "conversation.item.input_audio_transcription.delta" => {
            QwenServerEvent::InputTranscriptDelta(required_string(&value, "delta")?.to_owned())
        }
        "conversation.item.input_audio_transcription.completed" => {
            QwenServerEvent::InputTranscriptDone(required_string(&value, "transcript")?.to_owned())
        }
        "response.audio.delta" => {
            let encoded = required_string(&value, "delta")?;
            let audio = STANDARD
                .decode(encoded)
                .map_err(|_| "invalid base64 response audio")?;
            if audio.len() > MAX_AUDIO_CHUNK_BYTES {
                return Err("response audio chunk exceeds 256 KiB");
            }
            QwenServerEvent::ResponseAudio(audio)
        }
        "response.audio_transcript.delta" => {
            QwenServerEvent::ResponseTranscriptDelta(required_string(&value, "delta")?.to_owned())
        }
        "response.audio_transcript.done" => QwenServerEvent::ResponseTranscriptDone(
            required_string(&value, "transcript")?.to_owned(),
        ),
        "response.done" => {
            let status = value
                .pointer("/response/status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            QwenServerEvent::ResponseDone {
                cancelled: status == "cancelled",
            }
        }
        "error" => QwenServerEvent::Error {
            code: bounded_provider_text(
                value
                    .pointer("/error/code")
                    .and_then(Value::as_str)
                    .unwrap_or("provider_error"),
            ),
            message: bounded_provider_text(
                value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Qwen provider error"),
            ),
        },
        other => QwenServerEvent::Ignored {
            event_type: bounded_provider_text(other),
        },
    };
    Ok(event)
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, &'static str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or("Qwen server event is missing a required string")
}

fn bounded_provider_text(value: &str) -> String {
    value
        .chars()
        .filter(|value| !value.is_control())
        .take(512)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_audio_and_cancel_without_credentials() {
        let audio = append_audio_message(&[1, 2, 3, 4]).unwrap();
        assert_eq!(audio["type"], "input_audio_buffer.append");
        assert_eq!(audio["audio"], "AQIDBA==");
        assert_eq!(cancel_response_message()["type"], "response.cancel");
    }

    #[test]
    fn parses_audio_transcript_and_cancelled_response() {
        assert_eq!(
            parse_server_event(r#"{"type":"response.audio.delta","delta":"AQIDBA=="}"#).unwrap(),
            QwenServerEvent::ResponseAudio(vec![1, 2, 3, 4])
        );
        assert_eq!(
            parse_server_event(r#"{"type":"response.audio_transcript.delta","delta":"hello"}"#)
                .unwrap(),
            QwenServerEvent::ResponseTranscriptDelta("hello".into())
        );
        assert_eq!(
            parse_server_event(r#"{"type":"response.done","response":{"status":"cancelled"}}"#)
                .unwrap(),
            QwenServerEvent::ResponseDone { cancelled: true }
        );
    }

    #[test]
    fn bounds_untrusted_error_text() {
        let event = parse_server_event(&format!(
            r#"{{"type":"error","error":{{"code":"bad","message":"{}\nsecret"}}}}"#,
            "x".repeat(800)
        ))
        .unwrap();
        let QwenServerEvent::Error { message, .. } = event else {
            panic!("expected error");
        };
        assert_eq!(message.chars().count(), 512);
        assert!(!message.contains('\n'));
    }
}
