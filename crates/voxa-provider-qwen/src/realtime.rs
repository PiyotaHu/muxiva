use std::{error::Error, fmt};

use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::{header::AUTHORIZATION, HeaderValue},
    stream::MaybeTlsStream,
    Message, WebSocket,
};

use crate::protocol::{
    append_audio_message, cancel_response_message, parse_server_event, session_update_message,
    QwenServerEvent, QwenTurnDetection,
};

pub const QWEN_CREDENTIALS_RESOURCE: &str = "provider.qwen.credentials";

/// Non-serializable Qwen credentials intended for Voxa's ResourceStore.
pub struct QwenCredentials {
    api_key: Vec<u8>,
    workspace_id: String,
}

impl QwenCredentials {
    pub fn new(
        api_key: impl AsRef<[u8]>,
        workspace_id: impl Into<String>,
    ) -> Result<Self, QwenRealtimeError> {
        let api_key = api_key.as_ref();
        let workspace_id = workspace_id.into();
        if api_key.is_empty() || api_key.len() > 16 * 1024 {
            return Err(QwenRealtimeError::Configuration("invalid API key length"));
        }
        if workspace_id.is_empty()
            || workspace_id.len() > 255
            || !workspace_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(QwenRealtimeError::Configuration("invalid workspace ID"));
        }
        Ok(Self {
            api_key: api_key.to_vec(),
            workspace_id,
        })
    }

    fn authorization(&self) -> Result<HeaderValue, QwenRealtimeError> {
        let mut value = b"Bearer ".to_vec();
        value.extend_from_slice(&self.api_key);
        HeaderValue::from_bytes(&value)
            .map_err(|_| QwenRealtimeError::Configuration("API key is not a valid HTTP header"))
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
}

impl fmt::Debug for QwenCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QwenCredentials")
            .field("api_key", &"<redacted>")
            .field("workspace_id", &self.workspace_id)
            .finish()
    }
}

impl Drop for QwenCredentials {
    fn drop(&mut self) {
        self.api_key.fill(0);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QwenRealtimeConfig {
    pub endpoint: Option<String>,
    pub model: String,
    pub voice: String,
    pub instructions: String,
    pub turn_detection: QwenTurnDetection,
}

impl Default for QwenRealtimeConfig {
    fn default() -> Self {
        Self {
            endpoint: None,
            model: "qwen-audio-3.0-realtime-flash".into(),
            voice: "Cherry".into(),
            instructions: "You are a concise, helpful realtime voice assistant.".into(),
            turn_detection: QwenTurnDetection::SmartTurn,
        }
    }
}

impl QwenRealtimeConfig {
    fn endpoint(&self, workspace_id: &str) -> Result<String, QwenRealtimeError> {
        if let Some(endpoint) = &self.endpoint {
            return Ok(endpoint.clone());
        }
        if self.model.is_empty() || self.model.len() > 255 {
            return Err(QwenRealtimeError::Configuration("invalid model"));
        }
        Ok(format!(
            "wss://{workspace_id}.cn-beijing.maas.aliyuncs.com/api-ws/v1/realtime?model={}",
            self.model
        ))
    }
}

#[derive(Debug)]
pub enum QwenRealtimeError {
    Configuration(&'static str),
    Transport(String),
    Protocol(&'static str),
    Closed,
}

impl fmt::Display for QwenRealtimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => {
                write!(formatter, "invalid Qwen configuration: {message}")
            }
            Self::Transport(message) => write!(formatter, "Qwen transport failed: {message}"),
            Self::Protocol(message) => {
                write!(formatter, "invalid Qwen protocol message: {message}")
            }
            Self::Closed => formatter.write_str("Qwen realtime connection closed"),
        }
    }
}

impl Error for QwenRealtimeError {}

/// Authenticated synchronous WebSocket session used by the runtime adapter.
pub struct QwenRealtimeSocket {
    socket: WebSocket<MaybeTlsStream<std::net::TcpStream>>,
}

impl QwenRealtimeSocket {
    pub fn connect(
        credentials: &QwenCredentials,
        config: &QwenRealtimeConfig,
    ) -> Result<Self, QwenRealtimeError> {
        let endpoint = config.endpoint(credentials.workspace_id())?;
        let mut request = endpoint
            .into_client_request()
            .map_err(|error| QwenRealtimeError::Transport(safe_transport_error(&error)))?;
        request
            .headers_mut()
            .insert(AUTHORIZATION, credentials.authorization()?);
        let (mut socket, _) = connect(request)
            .map_err(|error| QwenRealtimeError::Transport(safe_transport_error(&error)))?;
        send_json(
            &mut socket,
            session_update_message(&config.voice, &config.instructions, config.turn_detection),
        )?;
        set_nonblocking(&mut socket, true)?;
        Ok(Self { socket })
    }

    pub fn send_audio(&mut self, pcm_s16le: &[u8]) -> Result<(), QwenRealtimeError> {
        let message = append_audio_message(pcm_s16le).map_err(QwenRealtimeError::Protocol)?;
        send_json(&mut self.socket, message)
    }

    pub fn cancel_response(&mut self) -> Result<(), QwenRealtimeError> {
        send_json(&mut self.socket, cancel_response_message())
    }

    pub fn read_event(&mut self) -> Result<QwenServerEvent, QwenRealtimeError> {
        loop {
            if let Some(event) = self.try_read_event()? {
                return Ok(event);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Reads one available event without waiting for network input.
    pub fn try_read_event(&mut self) -> Result<Option<QwenServerEvent>, QwenRealtimeError> {
        match self.socket.read() {
            Ok(Message::Text(text)) => parse_server_event(&text)
                .map(Some)
                .map_err(QwenRealtimeError::Protocol),
            Ok(Message::Ping(payload)) => {
                self.socket
                    .send(Message::Pong(payload))
                    .map_err(|error| QwenRealtimeError::Transport(safe_transport_error(&error)))?;
                Ok(None)
            }
            Ok(Message::Close(_)) => Err(QwenRealtimeError::Closed),
            Ok(Message::Binary(_) | Message::Pong(_) | Message::Frame(_)) => Ok(None),
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(QwenRealtimeError::Transport(safe_transport_error(&error))),
        }
    }
}

fn set_nonblocking(
    socket: &mut WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    nonblocking: bool,
) -> Result<(), QwenRealtimeError> {
    let result = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_nonblocking(nonblocking),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_nonblocking(nonblocking),
        _ => return Err(QwenRealtimeError::Configuration("unsupported TLS backend")),
    };
    result.map_err(|error| QwenRealtimeError::Transport(safe_transport_error(&error)))
}

fn send_json(
    socket: &mut WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    message: serde_json::Value,
) -> Result<(), QwenRealtimeError> {
    socket
        .send(Message::Text(message.to_string().into()))
        .map_err(|error| QwenRealtimeError::Transport(safe_transport_error(&error)))
}

fn safe_transport_error(error: &impl fmt::Display) -> String {
    error
        .to_string()
        .chars()
        .filter(|character| !character.is_control())
        .take(512)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, thread};

    use serde_json::Value;
    use tungstenite::{
        accept_hdr,
        handshake::server::{Request, Response},
    };

    use super::*;

    #[test]
    #[allow(clippy::result_large_err)]
    fn authenticates_sends_audio_and_cancels_against_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = accept_hdr(stream, |request: &Request, response: Response| {
                assert_eq!(request.headers()[AUTHORIZATION], "Bearer test-secret");
                Ok(response)
            })
            .unwrap();
            let session = read_json(&mut socket);
            assert_eq!(session["type"], "session.update");
            let audio = read_json(&mut socket);
            assert_eq!(audio["type"], "input_audio_buffer.append");
            assert_eq!(audio["audio"], "AQIDBA==");
            socket
                .send(Message::Text(
                    r#"{"type":"response.audio_transcript.delta","delta":"你好"}"#.into(),
                ))
                .unwrap();
            let cancel = read_json(&mut socket);
            assert_eq!(cancel["type"], "response.cancel");
        });

        let credentials = QwenCredentials::new("test-secret", "workspace-1").unwrap();
        let config = QwenRealtimeConfig {
            endpoint: Some(format!("ws://{address}/realtime")),
            ..QwenRealtimeConfig::default()
        };
        let mut client = QwenRealtimeSocket::connect(&credentials, &config).unwrap();
        client.send_audio(&[1, 2, 3, 4]).unwrap();
        assert_eq!(
            client.read_event().unwrap(),
            QwenServerEvent::ResponseTranscriptDelta("你好".into())
        );
        client.cancel_response().unwrap();
        server.join().unwrap();
    }

    fn read_json(socket: &mut WebSocket<std::net::TcpStream>) -> Value {
        match socket.read().unwrap() {
            Message::Text(text) => serde_json::from_str(&text).unwrap(),
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn debug_never_contains_api_key() {
        let credentials = QwenCredentials::new("very-secret", "workspace-1").unwrap();
        let rendered = format!("{credentials:?}");
        assert!(!rendered.contains("very-secret"));
        assert!(rendered.contains("<redacted>"));
    }
}
