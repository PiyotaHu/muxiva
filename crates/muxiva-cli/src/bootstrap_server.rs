use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const MAX_REQUEST_BYTES: usize = 16 * 1024;

pub struct BootstrapServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl BootstrapServer {
    pub fn start(
        listener: TcpListener,
        graph_id: String,
        client_session: Value,
        allowed_origins: Vec<String>,
        access_token: Option<String>,
    ) -> Result<Self, String> {
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot make client API non-blocking: {error}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("muxiva-client-api".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if let Err(error) = handle_connection(
                                stream,
                                &graph_id,
                                &client_session,
                                &allowed_origins,
                                access_token.as_deref(),
                            ) {
                                eprintln!("[MUXIVA][WARN][client-api.request] {error}");
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(error) => {
                            eprintln!("[MUXIVA][ERROR][client-api.accept] {error}");
                            break;
                        }
                    }
                }
            })
            .map_err(|error| format!("cannot start client API worker: {error}"))?;
        Ok(Self {
            address,
            stop,
            worker: Some(worker),
        })
    }

    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for BootstrapServer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Request {
    method: String,
    path: String,
    origin: Option<String>,
    authorization: Option<String>,
}

fn handle_connection(
    mut stream: TcpStream,
    graph_id: &str,
    client_session: &Value,
    allowed_origins: &[String],
    access_token: Option<&str>,
) -> io::Result<()> {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(message) => return write_response(&mut stream, "400 Bad Request", None, message),
    };
    let allowed_origin = match request.origin.as_deref() {
        Some(origin) if origin_allowed(origin, allowed_origins) => Some(origin),
        Some(_) => {
            return write_response(
                &mut stream,
                "403 Forbidden",
                None,
                r#"{"message":"browser origin is not allowed"}"#,
            )
        }
        None => None,
    };
    if request.method == "OPTIONS" {
        return write_response(&mut stream, "204 No Content", allowed_origin, "");
    }
    if request.method != "GET" {
        return write_response(
            &mut stream,
            "405 Method Not Allowed",
            allowed_origin,
            r#"{"message":"method not allowed"}"#,
        );
    }
    match request.path.as_str() {
        "/healthz" => write_response(
            &mut stream,
            "200 OK",
            allowed_origin,
            &serde_json::json!({
                "status": "ok",
                "mode": "headless",
                "graph_id": graph_id,
            })
            .to_string(),
        ),
        "/api/v1/client/session" => {
            if !authorized(request.authorization.as_deref(), access_token) {
                return write_response(
                    &mut stream,
                    "401 Unauthorized",
                    allowed_origin,
                    r#"{"message":"a valid Bearer token is required"}"#,
                );
            }
            let mut payload = client_session.clone();
            if let Some(object) = payload.as_object_mut() {
                object.insert(
                    "runtime".into(),
                    serde_json::json!({"mode":"headless", "graph_id":graph_id}),
                );
            }
            write_response(&mut stream, "200 OK", allowed_origin, &payload.to_string())
        }
        _ => write_response(
            &mut stream,
            "404 Not Found",
            allowed_origin,
            r#"{"message":"not found"}"#,
        ),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, &'static str> {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|_| r#"{"message":"cannot configure request timeout"}"#)?;
    let mut data = Vec::new();
    let mut buffer = [0_u8; 2048];
    while !data.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream
            .read(&mut buffer)
            .map_err(|_| r#"{"message":"cannot read request"}"#)?;
        if count == 0 {
            return Err(r#"{"message":"incomplete request"}"#);
        }
        data.extend_from_slice(&buffer[..count]);
        if data.len() > MAX_REQUEST_BYTES {
            return Err(r#"{"message":"request headers are too large"}"#);
        }
    }
    let text = std::str::from_utf8(&data).map_err(|_| r#"{"message":"invalid request"}"#)?;
    let mut lines = text.split("\r\n");
    let mut first = lines
        .next()
        .ok_or(r#"{"message":"missing request line"}"#)?
        .split_whitespace();
    let method = first.next().ok_or(r#"{"message":"missing method"}"#)?;
    let raw_path = first.next().ok_or(r#"{"message":"missing path"}"#)?;
    if first.next().is_none() || !raw_path.starts_with('/') {
        return Err(r#"{"message":"invalid request line"}"#);
    }
    let mut origin = None;
    let mut authorization = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("origin") {
            origin = Some(value.trim().to_owned());
        } else if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_owned());
        }
    }
    Ok(Request {
        method: method.to_owned(),
        path: raw_path.split('?').next().unwrap_or(raw_path).to_owned(),
        origin,
        authorization,
    })
}

fn origin_allowed(origin: &str, allowed_origins: &[String]) -> bool {
    allowed_origins
        .iter()
        .any(|allowed| allowed == "*" || allowed == origin)
}

fn authorized(header: Option<&str>, token: Option<&str>) -> bool {
    match token {
        None => true,
        Some(token) => header == Some(&format!("Bearer {token}")),
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    allowed_origin: Option<&str>,
    body: &str,
) -> io::Result<()> {
    let cors = allowed_origin
        .map(|origin| {
            format!(
                "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\nAccess-Control-Allow-Methods: GET, OPTIONS\r\n"
            )
        })
        .unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n{cors}Connection: close\r\n\r\n{body}",
        body.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(address: SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn client_session_is_cors_scoped_and_token_protected() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut server = BootstrapServer::start(
            listener,
            "voice".into(),
            serde_json::json!({"agora":{"channel":"room"}}),
            vec!["http://127.0.0.1:4173".into()],
            Some("secret".into()),
        )
        .unwrap();
        let unauthorized = request(
            server.address(),
            "GET /api/v1/client/session HTTP/1.1\r\nHost: localhost\r\nOrigin: http://127.0.0.1:4173\r\n\r\n",
        );
        assert!(unauthorized.starts_with("HTTP/1.1 401"));
        let allowed = request(
            server.address(),
            "GET /api/v1/client/session HTTP/1.1\r\nHost: localhost\r\nOrigin: http://127.0.0.1:4173\r\nAuthorization: Bearer secret\r\n\r\n",
        );
        assert!(allowed.starts_with("HTTP/1.1 200"));
        assert!(allowed.contains("Access-Control-Allow-Origin: http://127.0.0.1:4173"));
        assert!(allowed.contains("\"channel\":\"room\""));
        let rejected = request(
            server.address(),
            "GET /healthz HTTP/1.1\r\nHost: localhost\r\nOrigin: https://evil.example\r\n\r\n",
        );
        assert!(rejected.starts_with("HTTP/1.1 403"));
        server.stop();
    }
}
