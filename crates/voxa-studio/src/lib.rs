//! Minimal local-only Studio HTTP server. It serves no remote assets or CORS.
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
};
pub fn random_token() -> std::io::Result<String> {
    let mut b = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}
pub fn serve(listener: TcpListener, graph: PathBuf, token: String) -> std::io::Result<()> {
    for s in listener.incoming() {
        handle_connection(s?, &graph, &token)?;
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, graph: &PathBuf, token: &str) -> std::io::Result<()> {
    let mut bytes = [0_u8; 16 * 1024];
    let count = stream.read(&mut bytes)?;
    let request = String::from_utf8_lossy(&bytes[..count]);
    let (headers, body) = request.split_once("\r\n\r\n").unwrap_or((&request, ""));
    let mut lines = headers.lines();
    let request_line = lines.next().unwrap_or("");
    let authorized = lines.any(|line| line.trim() == format!("Authorization: Bearer {token}"));
    let (status, content_type, payload) = match request_line
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["GET", "/"] => ("200 OK", "text/html", INDEX.to_string()),
        ["GET", "/api/v1/schema/graph-v1"] if authorized => (
            "200 OK",
            "application/json",
            voxa_graph_json::GRAPH_V1_SCHEMA.to_string(),
        ),
        ["GET", "/api/v1/graph"] if authorized => (
            "200 OK",
            "application/json",
            fs::read_to_string(graph).unwrap_or_default(),
        ),
        ["POST", "/api/v1/graph/validate"] if authorized => {
            match voxa_graph_json::parse(body)
                .and_then(|document| voxa_graph_json::compile(&document).map(|_| document))
            {
                Ok(_) => ("200 OK", "application/json", "[]".into()),
                Err(errors) => (
                    "400 Bad Request",
                    "application/json",
                    serde_json::to_string(&errors).unwrap(),
                ),
            }
        }
        _ if !authorized => ("401 Unauthorized", "text/plain", "unauthorized".into()),
        _ => ("404 Not Found", "text/plain", "not found".into()),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'self'\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())
}

const INDEX: &str = "<!doctype html><meta charset=utf-8><title>Voxa Studio</title><h1>Voxa Studio</h1><pre id=o>Loading schema…</pre><script>let t=location.hash.slice(1);history.replaceState(null,'','/');fetch('/api/v1/schema/graph-v1',{headers:{Authorization:'Bearer '+t}}).then(r=>r.text()).then(x=>o.textContent=x)</script>";

#[cfg(test)]
mod tests {
    use super::handle_connection;
    use std::{
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        path::PathBuf,
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

    fn request(graph: PathBuf, token: &str, raw_request: String) -> Option<String> {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("SKIP Studio HTTP contract: sandbox denies socket binding");
                fs::remove_file(graph).unwrap();
                return None;
            }
            Err(error) => panic!("failed to bind Studio contract server: {error}"),
        };
        let address = listener.local_addr().unwrap();
        let token = token.to_owned();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_connection(stream, &graph, &token).unwrap();
            fs::remove_file(graph).unwrap();
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
    fn graph_api_rejects_missing_and_forged_bearer_tokens() {
        for authorization in ["", "Authorization: Bearer forged\r\n"] {
            let Some(response) = request(
                graph_path(),
                "expected-token",
                format!("GET /api/v1/graph HTTP/1.1\r\nHost: localhost\r\n{authorization}\r\n"),
            ) else {
                return;
            };
            assert!(response.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
            assert!(!response.contains("text-uppercase"));
            assert!(!response.contains("expected-token"));
        }
    }

    #[test]
    fn authorized_graph_and_validation_routes_share_graph_v1_contract() {
        let Some(graph_response) = request(
            graph_path(),
            "contract-token",
            "GET /api/v1/graph HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer contract-token\r\n\r\n".into(),
        ) else { return };
        assert!(graph_response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(graph_response.contains("\"version\":\"voxa.graph/v1\""));

        let invalid = r#"{"version":"voxa.graph/v1","graph_id":"broken","nodes":[],"edges":[],"unexpected":true}"#;
        let Some(validation_response) = request(
            graph_path(),
            "contract-token",
            format!(
                "POST /api/v1/graph/validate HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer contract-token\r\nContent-Length: {}\r\n\r\n{invalid}",
                invalid.len()
            ),
        ) else { return };
        assert!(validation_response.starts_with("HTTP/1.1 400 Bad Request\r\n"));
        assert!(validation_response.contains("VOXA-GRAPH-JSON"));
    }
}
