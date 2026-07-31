//! Minimal local-only Studio HTTP server. It serves no remote assets or CORS.
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
};
pub fn random_token() -> std::io::Result<String> {
    let mut b = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}
pub fn serve(listener: TcpListener, graph: PathBuf, token: String) -> std::io::Result<()> {
    for s in listener.incoming() {
        let mut s = s?;
        let mut bytes = [0_u8; 16 * 1024];
        let count = s.read(&mut bytes)?;
        let r = String::from_utf8_lossy(&bytes[..count]);
        let (h, b) = r.split_once("\r\n\r\n").unwrap_or((&r, ""));
        let mut l = h.lines();
        let f = l.next().unwrap_or("");
        let ok = l.any(|x| x.trim() == format!("Authorization: Bearer {token}"));
        let (a, c, p) = match f.split_whitespace().take(2).collect::<Vec<_>>().as_slice() {
            ["GET", "/"] => ("200 OK", "text/html", INDEX.to_string()),
            ["GET", "/api/v1/schema/graph-v1"] if ok => (
                "200 OK",
                "application/json",
                voxa_graph_json::GRAPH_V1_SCHEMA.to_string(),
            ),
            ["GET", "/api/v1/graph"] if ok => (
                "200 OK",
                "application/json",
                fs::read_to_string(&graph).unwrap_or_default(),
            ),
            ["POST", "/api/v1/graph/validate"] if ok => match voxa_graph_json::parse(b)
                .and_then(|d| voxa_graph_json::compile(&d).map(|_| d))
            {
                Ok(_) => ("200 OK", "application/json", "[]".into()),
                Err(e) => (
                    "400 Bad Request",
                    "application/json",
                    serde_json::to_string(&e).unwrap(),
                ),
            },
            _ if !ok => ("401 Unauthorized", "text/plain", "unauthorized".into()),
            _ => ("404 Not Found", "text/plain", "not found".into()),
        };
        let out=format!("HTTP/1.1 {a}\r\nContent-Type: {c}\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'self'\r\nConnection: close\r\n\r\n{p}",p.len());
        s.write_all(out.as_bytes())?;
    }
    Ok(())
}
const INDEX:&str="<!doctype html><meta charset=utf-8><title>Voxa Studio</title><h1>Voxa Studio</h1><pre id=o>Loading schema…</pre><script>let t=location.hash.slice(1);history.replaceState(null,'','/');fetch('/api/v1/schema/graph-v1',{headers:{Authorization:'Bearer '+t}}).then(r=>r.text()).then(x=>o.textContent=x)</script>";
