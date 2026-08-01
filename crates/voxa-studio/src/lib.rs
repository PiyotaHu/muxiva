//! Local-only Voxa Graph Studio server with bundled, dependency-free assets.

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use voxa_graph_json::{GraphDiagnostic, GraphDocument, MAX_DOCUMENT_BYTES};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const INDEX: &str = include_str!("assets/index.html");
const STYLES: &str = include_str!("assets/studio.css");
const SCRIPT: &str = include_str!("assets/studio.js");
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0_u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn serve(listener: TcpListener, graph: PathBuf, token: String) -> std::io::Result<()> {
    for stream in listener.incoming() {
        let stream = stream?;
        if let Err(error) = handle_connection(stream, &graph, &token) {
            eprintln!("Studio connection error: {error}");
        }
    }
    Ok(())
}

struct HttpRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    body: String,
}

struct RequestError {
    status: &'static str,
    message: &'static str,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, RequestError> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).map_err(|_| RequestError {
            status: "400 Bad Request",
            message: "failed to read request",
        })?;
        if count == 0 {
            return Err(RequestError {
                status: "400 Bad Request",
                message: "incomplete request",
            });
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let end = position + 4;
            if end > MAX_HEADER_BYTES {
                return Err(RequestError {
                    status: "431 Request Header Fields Too Large",
                    message: "request headers exceed 16 KiB",
                });
            }
            break end;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(RequestError {
                status: "431 Request Header Fields Too Large",
                message: "request headers exceed 16 KiB",
            });
        }
    };

    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| RequestError {
        status: "400 Bad Request",
        message: "request headers must be UTF-8",
    })?;
    let mut lines = headers.lines();
    let mut request_line = lines.next().unwrap_or_default().split_whitespace();
    let method = request_line.next().unwrap_or_default().to_owned();
    let path = request_line
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_owned();
    if method.is_empty() || path.is_empty() {
        return Err(RequestError {
            status: "400 Bad Request",
            message: "invalid request line",
        });
    }

    let mut content_length = 0_usize;
    let mut authorization = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                content_length = value.trim().parse().map_err(|_| RequestError {
                    status: "400 Bad Request",
                    message: "invalid Content-Length",
                })?;
            }
            "authorization" => authorization = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    if content_length > MAX_DOCUMENT_BYTES {
        return Err(RequestError {
            status: "413 Payload Too Large",
            message: "graph document exceeds 1 MiB",
        });
    }

    let expected = header_end.saturating_add(content_length);
    while bytes.len() < expected {
        let count = stream.read(&mut buffer).map_err(|_| RequestError {
            status: "400 Bad Request",
            message: "failed to read request body",
        })?;
        if count == 0 {
            return Err(RequestError {
                status: "400 Bad Request",
                message: "incomplete request body",
            });
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > expected {
            bytes.truncate(expected);
        }
    }
    let body =
        String::from_utf8(bytes[header_end..expected].to_vec()).map_err(|_| RequestError {
            status: "400 Bad Request",
            message: "request body must be UTF-8",
        })?;
    Ok(HttpRequest {
        method,
        path,
        authorization,
        body,
    })
}

fn handle_connection(mut stream: TcpStream, graph: &Path, token: &str) -> std::io::Result<()> {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            return write_response(&mut stream, error.status, "text/plain", error.message)
        }
    };
    let authorized = request.authorization.as_deref() == Some(&format!("Bearer {token}"));
    let (status, content_type, payload) = route(&request, graph, authorized);
    write_response(&mut stream, status, content_type, &payload)
}

fn route(
    request: &HttpRequest,
    graph: &Path,
    authorized: bool,
) -> (&'static str, &'static str, String) {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => ("200 OK", "text/html; charset=utf-8", INDEX.to_owned()),
        ("GET", "/assets/studio.css") => ("200 OK", "text/css; charset=utf-8", STYLES.to_owned()),
        ("GET", "/assets/studio.js") => (
            "200 OK",
            "text/javascript; charset=utf-8",
            SCRIPT.to_owned(),
        ),
        _ if !authorized => (
            "401 Unauthorized",
            "text/plain; charset=utf-8",
            "unauthorized".into(),
        ),
        ("GET", "/api/v1/schema/graph-v1") => (
            "200 OK",
            "application/json",
            voxa_graph_json::GRAPH_V1_SCHEMA.to_owned(),
        ),
        ("GET", "/api/v1/registry/nodes") => (
            "200 OK",
            "application/json",
            serde_json::to_string(&voxa_graph_json::builtin_node_catalog())
                .unwrap_or_else(|_| "[]".into()),
        ),
        ("GET", "/api/v1/graph") => match fs::read_to_string(graph) {
            Ok(document) => ("200 OK", "application/json", document),
            Err(error) => (
                "500 Internal Server Error",
                "application/json",
                json_message(&format!("failed to read graph: {error}")),
            ),
        },
        ("GET", "/api/v1/studio") => {
            let payload = serde_json::json!({
                "graph_path": graph.display().to_string(),
                "max_document_bytes": MAX_DOCUMENT_BYTES,
                "writable": fs::metadata(graph).map(|metadata| !metadata.permissions().readonly()).unwrap_or(false),
            });
            ("200 OK", "application/json", payload.to_string())
        }
        ("POST", "/api/v1/graph/validate") => match validate(&request.body) {
            Ok(_) => ("200 OK", "application/json", "[]".into()),
            Err(errors) => diagnostics_response(errors),
        },
        ("PUT", "/api/v1/graph") => match validate(&request.body) {
            Ok(document) => match save_graph(graph, &document) {
                Ok(bytes) => (
                    "200 OK",
                    "application/json",
                    serde_json::json!({"saved": true, "bytes": bytes}).to_string(),
                ),
                Err(error) => (
                    "500 Internal Server Error",
                    "application/json",
                    json_message(&format!("failed to save graph: {error}")),
                ),
            },
            Err(errors) => diagnostics_response(errors),
        },
        _ => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found".into(),
        ),
    }
}

fn validate(input: &str) -> Result<GraphDocument, Vec<GraphDiagnostic>> {
    voxa_graph_json::parse(input)
        .and_then(|document| voxa_graph_json::compile(&document).map(|_| document))
}

fn diagnostics_response(errors: Vec<GraphDiagnostic>) -> (&'static str, &'static str, String) {
    (
        "400 Bad Request",
        "application/json",
        serde_json::to_string(&errors).unwrap_or_else(|_| "[]".into()),
    )
}

fn save_graph(path: &Path, document: &GraphDocument) -> std::io::Result<usize> {
    let mut payload = serde_json::to_string_pretty(document)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    payload.push('\n');
    let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary = temporary_path(path, sequence);
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(payload.as_bytes())?;
        file.sync_all()?;
        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&temporary, metadata.permissions())?;
        }
        fs::rename(&temporary, path)?;
        Ok(payload.len())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path, sequence: u64) -> PathBuf {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("graph.json");
    path.with_file_name(format!(
        ".{filename}.studio-{}-{sequence}.tmp",
        std::process::id()
    ))
}

fn json_message(message: &str) -> String {
    serde_json::json!({"message": message}).to_string()
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    payload: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::handle_connection;
    use std::{
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

    fn graph_path() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "voxa-studio-contract-{}-{}.json",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(
            &path,
            include_str!("../../../examples/graphs/text-uppercase.v1.json"),
        )
        .unwrap();
        path
    }

    fn request(graph: &Path, token: &str, raw_request: String) -> Option<String> {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("SKIP Studio HTTP contract: sandbox denies socket binding");
                return None;
            }
            Err(error) => panic!("failed to bind Studio contract server: {error}"),
        };
        let address = listener.local_addr().unwrap();
        let token = token.to_owned();
        let graph = graph.to_path_buf();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_connection(stream, &graph, &token).unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(raw_request.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        Some(response)
    }

    #[test]
    fn bundled_page_uses_external_assets_and_strict_csp() {
        let graph = graph_path();
        let Some(response) = request(
            &graph,
            "token",
            "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n".into(),
        ) else {
            return;
        };
        fs::remove_file(graph).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("script-src 'self'"));
        assert!(response.contains("/assets/studio.js"));
        assert!(!response.contains("<script>"));
    }

    #[test]
    fn graph_api_rejects_missing_and_forged_bearer_tokens() {
        for authorization in ["", "Authorization: Bearer forged\r\n"] {
            let graph = graph_path();
            let Some(response) = request(
                &graph,
                "expected-token",
                format!("GET /api/v1/graph HTTP/1.1\r\nHost: localhost\r\n{authorization}\r\n"),
            ) else {
                return;
            };
            fs::remove_file(graph).unwrap();
            assert!(response.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
            assert!(!response.contains("text-uppercase"));
            assert!(!response.contains("expected-token"));
        }
    }

    #[test]
    fn authorized_node_catalog_comes_from_the_runtime_registry() {
        let graph = graph_path();
        let Some(response) = request(
            &graph,
            "catalog-token",
            "GET /api/v1/registry/nodes HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer catalog-token\r\n\r\n".into(),
        ) else {
            return;
        };
        fs::remove_file(graph).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("builtin.text_source"));
        assert!(response.contains("factory_version"));
        assert!(response.contains("config_schema"));
    }

    #[test]
    fn authorized_validation_and_atomic_save_share_graph_v1_contract() {
        let graph = graph_path();
        let invalid = r#"{"version":"voxa.graph/v1","graph_id":"broken","nodes":[],"edges":[],"unexpected":true}"#;
        let Some(validation_response) = request(
            &graph,
            "contract-token",
            format!(
                "POST /api/v1/graph/validate HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer contract-token\r\nContent-Length: {}\r\n\r\n{invalid}",
                invalid.len()
            ),
        ) else {
            return;
        };
        assert!(validation_response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert!(validation_response.contains("VOXA-GRAPH-JSON"));

        let original = fs::read_to_string(&graph).unwrap();
        let Some(invalid_save_response) = request(
            &graph,
            "contract-token",
            format!(
                "PUT /api/v1/graph HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer contract-token\r\nContent-Length: {}\r\n\r\n{invalid}",
                invalid.len()
            ),
        ) else {
            return;
        };
        assert!(invalid_save_response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert_eq!(fs::read_to_string(&graph).unwrap(), original);

        let mut saved: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&graph).unwrap()).unwrap();
        saved["graph_id"] = "studio-saved".into();
        let saved = serde_json::to_string(&saved).unwrap();
        let Some(save_response) = request(
            &graph,
            "contract-token",
            format!(
                "PUT /api/v1/graph HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer contract-token\r\nContent-Length: {}\r\n\r\n{saved}",
                saved.len()
            ),
        ) else {
            return;
        };
        assert!(save_response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(fs::read_to_string(&graph).unwrap().contains("studio-saved"));
        fs::remove_file(graph).unwrap();
    }
}
