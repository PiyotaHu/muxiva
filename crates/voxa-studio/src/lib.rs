//! Local-only Voxa Graph Studio server with bundled, dependency-free assets.

mod node_library;

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use voxa_core::{
    start_registered_runtime_with_resources, EdgePolicies, GraphRuntime, ResourceKey,
    ResourceStore, RuntimeOptions, RuntimeWaitError,
};
use voxa_graph_json::{GraphDiagnostic, GraphDocument, MAX_DOCUMENT_BYTES};
use voxa_types::{EdgeId, NodeId};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const INDEX: &str = include_str!("assets/index.html");
const STYLES: &str = include_str!("assets/studio.css");
const RUNTIME_STYLES: &str = include_str!("assets/runtime.css");
const NODE_LAB_STYLES: &str = include_str!("assets/node-lab.css");
const SCRIPT: &str = include_str!("assets/studio.js");
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct RuntimeSession {
    id: u64,
    graph_id: String,
    node_ids: Vec<NodeId>,
    edge_ids: Vec<EdgeId>,
    runtime: GraphRuntime,
    started: Instant,
    stop_requested: bool,
}

struct StudioRuntime {
    next_session: AtomicU64,
    session: Mutex<Option<RuntimeSession>>,
    providers: Mutex<ProviderConfiguration>,
}

impl Default for StudioRuntime {
    fn default() -> Self {
        Self {
            next_session: AtomicU64::new(0),
            session: Mutex::new(None),
            providers: Mutex::new(ProviderConfiguration::from_environment()),
        }
    }
}

#[derive(Default)]
struct ProviderConfiguration {
    dashscope_api_key: SecretValue,
    dashscope_workspace_id: String,
    dashscope_region: String,
    qwen_realtime_model: String,
    agora_app_id: String,
    agora_channel: String,
    agora_user_token: SecretValue,
    agora_bot_token: SecretValue,
}

impl ProviderConfiguration {
    fn from_environment() -> Self {
        let mut value = Self::default();
        set_secret_from_env("DASHSCOPE_API_KEY", &mut value.dashscope_api_key);
        set_text_from_env("DASHSCOPE_WORKSPACE_ID", &mut value.dashscope_workspace_id);
        set_text_from_env("VOXA_QWEN_REGION", &mut value.dashscope_region);
        set_text_from_env("VOXA_QWEN_MODEL", &mut value.qwen_realtime_model);
        set_text_from_env("VOXA_AGORA_APP_ID", &mut value.agora_app_id);
        set_text_from_env("VOXA_AGORA_CHANNEL", &mut value.agora_channel);
        set_secret_from_env("VOXA_AGORA_USER_TOKEN", &mut value.agora_user_token);
        set_secret_from_env("VOXA_AGORA_BOT_TOKEN", &mut value.agora_bot_token);
        value
    }
}

fn set_text_from_env(name: &str, target: &mut String) {
    if let Some(value) = std::env::var_os(name).and_then(|value| value.into_string().ok()) {
        target.push_str(value.trim());
    }
}

fn set_secret_from_env(name: &str, target: &mut SecretValue) {
    if let Some(value) = std::env::var_os(name).and_then(|value| value.into_string().ok()) {
        target.replace(value.trim());
    }
}

#[derive(Default)]
struct SecretValue(Vec<u8>);

impl SecretValue {
    fn replace(&mut self, value: &str) {
        self.0.fill(0);
        self.0.clear();
        self.0.extend_from_slice(value.as_bytes());
    }

    fn is_set(&self) -> bool {
        !self.0.is_empty()
    }

    fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretValue {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0_u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn serve(listener: TcpListener, graph: PathBuf, token: String) -> std::io::Result<()> {
    let runtime = StudioRuntime::default();
    for stream in listener.incoming() {
        let stream = stream?;
        if let Err(error) = handle_connection(stream, &graph, &token, &runtime) {
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

fn handle_connection(
    mut stream: TcpStream,
    graph: &Path,
    token: &str,
    runtime: &StudioRuntime,
) -> std::io::Result<()> {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            return write_response(&mut stream, error.status, "text/plain", error.message)
        }
    };
    let authorized = request.authorization.as_deref() == Some(&format!("Bearer {token}"));
    let (status, content_type, payload) = route(&request, graph, authorized, runtime);
    write_response(&mut stream, status, content_type, &payload)
}

fn route(
    request: &HttpRequest,
    graph: &Path,
    authorized: bool,
    runtime: &StudioRuntime,
) -> (&'static str, &'static str, String) {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => ("200 OK", "text/html; charset=utf-8", INDEX.to_owned()),
        ("GET", "/assets/studio.css") => ("200 OK", "text/css; charset=utf-8", STYLES.to_owned()),
        ("GET", "/assets/runtime.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            RUNTIME_STYLES.to_owned(),
        ),
        ("GET", "/assets/node-lab.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            NODE_LAB_STYLES.to_owned(),
        ),
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
        ("GET", "/api/v1/registry/nodes") => catalog_response(graph),
        ("GET", "/api/v1/node-library") => match node_library::list(graph) {
            Ok(packages) => (
                "200 OK",
                "application/json",
                serde_json::to_string(&packages).unwrap_or_else(|_| "[]".into()),
            ),
            Err(error) => (
                "500 Internal Server Error",
                "application/json",
                json_message(&format!("failed to read the project Node Library: {error}")),
            ),
        },
        ("PUT", "/api/v1/node-library") => match node_library::save(graph, &request.body) {
            Ok(package) => (
                "200 OK",
                "application/json",
                serde_json::to_string(&package).unwrap_or_else(|_| "{}".into()),
            ),
            Err(node_library::SaveError::Invalid(message)) => (
                "400 Bad Request",
                "application/json",
                json_message(&message),
            ),
            Err(node_library::SaveError::Io(error)) => (
                "500 Internal Server Error",
                "application/json",
                json_message(&format!("failed to save the Node package: {error}")),
            ),
        },
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
        ("GET", "/api/v1/providers") => provider_status(runtime),
        ("PUT", "/api/v1/providers") => update_providers(runtime, &request.body),
        ("POST", "/api/v1/graph/validate") => match validate(&request.body, graph) {
            Ok(_) => ("200 OK", "application/json", "[]".into()),
            Err(errors) => diagnostics_response(errors),
        },
        ("GET", "/api/v1/runtime") => ("200 OK", "application/json", runtime_snapshot(runtime)),
        ("POST", "/api/v1/runtime/start") => start_runtime(runtime, &request.body, graph),
        ("POST", "/api/v1/runtime/stop") => stop_runtime(runtime),
        ("PUT", "/api/v1/graph") => match validate(&request.body, graph) {
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

fn provider_status(runtime: &StudioRuntime) -> (&'static str, &'static str, String) {
    let providers = runtime
        .providers
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let dashscope_key_set = providers.dashscope_api_key.is_set();
    let agora_user_token_set = providers.agora_user_token.is_set();
    let agora_bot_token_set = providers.agora_bot_token.is_set();
    let value = serde_json::json!({
        "dashscope": {
            "configured": dashscope_key_set && !providers.dashscope_workspace_id.is_empty(),
            "api_key_set": dashscope_key_set,
            "workspace_id": providers.dashscope_workspace_id,
            "region": defaulted(&providers.dashscope_region, "cn-beijing"),
            "realtime_model": defaulted(&providers.qwen_realtime_model, "qwen-audio-3.0-realtime-flash"),
        },
        "agora": {
            "configured": !providers.agora_app_id.is_empty()
                && !providers.agora_channel.is_empty()
                && agora_user_token_set
                && agora_bot_token_set,
            "app_id": providers.agora_app_id,
            "channel": providers.agora_channel,
            "user_token_set": agora_user_token_set,
            "bot_token_set": agora_bot_token_set,
        },
        "storage": "process-memory",
    });
    ("200 OK", "application/json", value.to_string())
}

fn update_providers(runtime: &StudioRuntime, input: &str) -> (&'static str, &'static str, String) {
    let value: serde_json::Value = match serde_json::from_str(input) {
        Ok(value) => value,
        Err(_) => {
            return (
                "400 Bad Request",
                "application/json",
                json_message("provider configuration must be valid JSON"),
            )
        }
    };
    let Some(root) = value.as_object() else {
        return (
            "400 Bad Request",
            "application/json",
            json_message("provider configuration must be an object"),
        );
    };
    let mut providers = runtime
        .providers
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(value) = root.get("dashscope") {
        let Some(object) = value.as_object() else {
            return provider_field_error("dashscope must be an object");
        };
        if let Err(message) = apply_secret(object, "api_key", &mut providers.dashscope_api_key)
            .and_then(|_| {
                apply_text(
                    object,
                    "workspace_id",
                    &mut providers.dashscope_workspace_id,
                    256,
                )
            })
            .and_then(|_| apply_text(object, "region", &mut providers.dashscope_region, 64))
            .and_then(|_| {
                apply_text(
                    object,
                    "realtime_model",
                    &mut providers.qwen_realtime_model,
                    128,
                )
            })
        {
            return provider_field_error(message);
        }
    }
    if let Some(value) = root.get("agora") {
        let Some(object) = value.as_object() else {
            return provider_field_error("agora must be an object");
        };
        if let Err(message) = apply_text(object, "app_id", &mut providers.agora_app_id, 256)
            .and_then(|_| apply_text(object, "channel", &mut providers.agora_channel, 256))
            .and_then(|_| apply_secret(object, "user_token", &mut providers.agora_user_token))
            .and_then(|_| apply_secret(object, "bot_token", &mut providers.agora_bot_token))
        {
            return provider_field_error(message);
        }
    }
    drop(providers);
    provider_status(runtime)
}

fn defaulted<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

fn apply_text(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &'static str,
    target: &mut String,
    maximum: usize,
) -> Result<(), &'static str> {
    let Some(value) = object.get(name) else {
        return Ok(());
    };
    let Some(value) = value.as_str() else {
        return Err("provider fields must be strings");
    };
    let value = value.trim();
    if value.len() > maximum {
        return Err("provider field exceeds its size limit");
    }
    target.clear();
    target.push_str(value);
    Ok(())
}

fn apply_secret(
    object: &serde_json::Map<String, serde_json::Value>,
    name: &'static str,
    target: &mut SecretValue,
) -> Result<(), &'static str> {
    let Some(value) = object.get(name) else {
        return Ok(());
    };
    let Some(value) = value.as_str() else {
        return Err("provider secrets must be strings");
    };
    if value.len() > 16 * 1024 {
        return Err("provider secret exceeds 16 KiB");
    }
    target.replace(value.trim());
    Ok(())
}

fn provider_field_error(message: &str) -> (&'static str, &'static str, String) {
    ("400 Bad Request", "application/json", json_message(message))
}

fn project_registry(graph: &Path) -> Result<voxa_core::NodeRegistry, String> {
    let mut registry = voxa_graph_json::builtin_registry();
    voxa_provider_qwen::register_qwen_nodes(&mut registry)
        .map_err(|error| format!("failed to register Qwen provider Nodes: {error}"))?;
    node_library::register_project_nodes(graph, &mut registry)?;
    Ok(registry)
}

fn catalog_response(graph: &Path) -> (&'static str, &'static str, String) {
    match project_registry(graph) {
        Ok(registry) => (
            "200 OK",
            "application/json",
            serde_json::to_string(&voxa_graph_json::node_catalog(&registry))
                .unwrap_or_else(|_| "[]".into()),
        ),
        Err(error) => (
            "400 Bad Request",
            "application/json",
            json_message(&format!("invalid project Node Library: {error}")),
        ),
    }
}

fn validate(input: &str, graph_path: &Path) -> Result<GraphDocument, Vec<GraphDiagnostic>> {
    let registry = project_registry(graph_path).map_err(|message| {
        vec![GraphDiagnostic {
            code: "VOXA-STUDIO-NODE-LIBRARY".into(),
            message,
            pointer: "/.voxa/nodes".into(),
        }]
    })?;
    voxa_graph_json::parse(input).and_then(|document| {
        voxa_graph_json::compile_with_registry(&document, &registry).map(|_| document)
    })
}

fn start_runtime(
    state: &StudioRuntime,
    input: &str,
    graph_path: &Path,
) -> (&'static str, &'static str, String) {
    let document = match voxa_graph_json::parse(input) {
        Ok(document) => document,
        Err(errors) => return diagnostics_response(errors),
    };
    let registry = match project_registry(graph_path) {
        Ok(registry) => registry,
        Err(error) => {
            return (
                "400 Bad Request",
                "application/json",
                json_message(&format!("invalid project Node Library: {error}")),
            )
        }
    };
    let graph = match voxa_graph_json::compile_with_registry(&document, &registry) {
        Ok(graph) => graph,
        Err(errors) => return diagnostics_response(errors),
    };
    let mut session = state
        .session
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if session.as_ref().is_some_and(RuntimeSession::is_active) {
        return (
            "409 Conflict",
            "application/json",
            json_message("a Studio runtime session is already active"),
        );
    }
    let graph_id = graph.graph_id().as_str().to_owned();
    let node_ids = graph
        .nodes()
        .iter()
        .map(|node| node.descriptor().node_id().clone())
        .collect();
    let edge_ids = graph
        .edges()
        .iter()
        .map(|edge| edge.edge_id().clone())
        .collect();
    let resources = match provider_resources(state) {
        Ok(resources) => resources,
        Err(message) => {
            return (
                "400 Bad Request",
                "application/json",
                json_message(&message),
            )
        }
    };
    let runtime = match start_registered_runtime_with_resources(
        graph,
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
        resources,
    ) {
        Ok(runtime) => runtime,
        Err(error) => {
            return (
                "500 Internal Server Error",
                "application/json",
                json_message(&format!("failed to start graph runtime: {error}")),
            )
        }
    };
    let id = state.next_session.fetch_add(1, Ordering::Relaxed) + 1;
    *session = Some(RuntimeSession {
        id,
        graph_id,
        node_ids,
        edge_ids,
        runtime,
        started: Instant::now(),
        stop_requested: false,
    });
    (
        "201 Created",
        "application/json",
        session_snapshot(session.as_ref().expect("installed session")).to_string(),
    )
}

fn provider_resources(state: &StudioRuntime) -> Result<ResourceStore, String> {
    let resources = ResourceStore::new();
    let providers = state
        .providers
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if providers.dashscope_api_key.is_set() && !providers.dashscope_workspace_id.is_empty() {
        let credentials = voxa_provider_qwen::QwenCredentials::new(
            providers.dashscope_api_key.as_bytes(),
            providers.dashscope_workspace_id.clone(),
        )
        .map_err(|error| error.to_string())?;
        let key = ResourceKey::new(voxa_provider_qwen::QWEN_CREDENTIALS_RESOURCE)
            .map_err(|error| error.to_string())?;
        resources
            .insert(key, std::sync::Arc::new(credentials))
            .map_err(|error| error.to_string())?;
    }
    Ok(resources)
}

fn stop_runtime(state: &StudioRuntime) -> (&'static str, &'static str, String) {
    let mut session = state
        .session
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(session) = session.as_mut() else {
        return (
            "200 OK",
            "application/json",
            serde_json::json!({"accepted": false, "status": "idle"}).to_string(),
        );
    };
    let accepted = session.runtime.stop();
    session.stop_requested = true;
    let mut snapshot = session_snapshot(session);
    snapshot["accepted"] = accepted.into();
    ("200 OK", "application/json", snapshot.to_string())
}

fn runtime_snapshot(state: &StudioRuntime) -> String {
    let session = state
        .session
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    session.as_ref().map_or_else(
        || serde_json::json!({"status": "idle", "session_id": null}).to_string(),
        |session| session_snapshot(session).to_string(),
    )
}

impl RuntimeSession {
    fn is_active(&self) -> bool {
        matches!(
            self.runtime.wait(Duration::ZERO),
            Err(RuntimeWaitError::Timeout(_))
        )
    }
}

fn session_snapshot(session: &RuntimeSession) -> serde_json::Value {
    let (status, active_nodes, worker_total, terminal) = match session.runtime.wait(Duration::ZERO)
    {
        Ok(summary) => (
            "completed",
            Vec::new(),
            Some(summary.worker_total()),
            serde_json::json!({"kind": "success"}),
        ),
        Err(RuntimeWaitError::Timeout(diagnostics)) => (
            if session.stop_requested {
                "stopping"
            } else {
                "running"
            },
            diagnostics
                .active_nodes()
                .iter()
                .map(|node| node.as_str().to_owned())
                .collect(),
            None,
            serde_json::Value::Null,
        ),
        Err(RuntimeWaitError::Aborted(reason)) => (
            if session.stop_requested {
                "stopped"
            } else {
                "aborted"
            },
            Vec::new(),
            None,
            serde_json::json!({
                "kind": if session.stop_requested { "cancelled" } else { "failure" },
                "code": reason.root().code(),
                "message": reason.root().message(),
                "node_id": reason.node_id().map(NodeId::as_str),
                "category": format!("{:?}", reason.category()).to_lowercase(),
                "stage": format!("{:?}", reason.stage()).to_lowercase(),
            }),
        ),
    };
    let nodes = session
        .node_ids
        .iter()
        .filter_map(|node_id| session.runtime.node_metrics(node_id))
        .map(|metrics| {
            serde_json::json!({
                "node_id": metrics.node_id().as_str(),
                "prepare_total": metrics.prepare_total(),
                "process_total": metrics.process_total(),
                "signal_total": metrics.signal_total(),
                "finish_total": metrics.finish_total(),
                "abort_total": metrics.abort_total(),
                "error_total": metrics.error_total(),
                "panic_total": metrics.panic_total(),
                "callback_duration_ns": metrics.callback_duration_ns(),
                "max_callback_duration_ns": metrics.max_callback_duration_ns(),
            })
        })
        .collect::<Vec<_>>();
    let edges = session
        .edge_ids
        .iter()
        .filter_map(|edge_id| session.runtime.edge_metrics(edge_id))
        .map(|metrics| {
            serde_json::json!({
                "edge_id": metrics.edge_id().as_str(),
                "queue_capacity": metrics.queue_capacity(),
                "queue_len": metrics.queue_len(),
                "high_watermark": metrics.high_watermark(),
                "enqueue_total": metrics.enqueue_total(),
                "dequeue_total": metrics.dequeue_total(),
                "drop_total": metrics.drop_total(),
                "signal_total": metrics.signal_total(),
                "full_total": metrics.full_total(),
                "blocked_duration_ns": metrics.blocked_duration_ns(),
                "oldest_frame_age_ns": metrics.oldest_frame_age_ns(),
                "latest_error_reason": metrics.latest_error_reason(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "session_id": session.id,
        "graph_id": session.graph_id,
        "status": status,
        "runtime_state": format!("{:?}", session.runtime.state()).to_lowercase(),
        "elapsed_ms": u64::try_from(session.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "worker_total": worker_total,
        "active_nodes": active_nodes,
        "stop_requested": session.stop_requested,
        "nodes": nodes,
        "edges": edges,
        "terminal": terminal,
    })
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
    use super::{handle_connection, route, HttpRequest, StudioRuntime};
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
            handle_connection(stream, &graph, &token, &StudioRuntime::default()).unwrap();
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
        for path in ["/api/v1/graph", "/api/v1/runtime"] {
            for authorization in ["", "Authorization: Bearer forged\r\n"] {
                let graph = graph_path();
                let Some(response) = request(
                    &graph,
                    "expected-token",
                    format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{authorization}\r\n"),
                ) else {
                    return;
                };
                fs::remove_file(graph).unwrap();
                assert!(response.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
                assert!(!response.contains("text-uppercase"));
                assert!(!response.contains("expected-token"));
            }
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

    #[test]
    fn runtime_api_starts_observes_and_retains_terminal_metrics() {
        let graph = graph_path();
        let body = fs::read_to_string(&graph).unwrap();
        let runtime = StudioRuntime::default();
        let start = HttpRequest {
            method: "POST".into(),
            path: "/api/v1/runtime/start".into(),
            authorization: None,
            body,
        };
        let (status, _, payload) = route(&start, &graph, true, &runtime);
        assert_eq!(status, "201 Created");
        assert!(payload.contains("session_id"));

        let snapshot = HttpRequest {
            method: "GET".into(),
            path: "/api/v1/runtime".into(),
            authorization: None,
            body: String::new(),
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let (_, _, payload) = route(&snapshot, &graph, true, &runtime);
            let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
            if value["status"] == "completed" {
                assert_eq!(value["nodes"].as_array().unwrap().len(), 3);
                assert_eq!(value["edges"].as_array().unwrap().len(), 2);
                assert_eq!(value["edges"][0]["enqueue_total"], 1);
                assert_eq!(value["nodes"][0]["prepare_total"], 1);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "runtime did not complete"
            );
            thread::yield_now();
        }
        fs::remove_file(graph).unwrap();
    }

    #[test]
    fn provider_configuration_never_echoes_or_persists_secrets() {
        let graph = graph_path();
        let original = fs::read_to_string(&graph).unwrap();
        let runtime = StudioRuntime::default();
        let api_key = "dashscope-private-test-key";
        let user_token = "agora-private-user-token";
        let bot_token = "agora-private-bot-token";
        let update = HttpRequest {
            method: "PUT".into(),
            path: "/api/v1/providers".into(),
            authorization: None,
            body: serde_json::json!({
                "dashscope": {
                    "api_key": api_key,
                    "workspace_id": "workspace-test",
                    "region": "cn-beijing",
                    "realtime_model": "qwen-audio-3.0-realtime-flash"
                },
                "agora": {
                    "app_id": "app-test",
                    "channel": "voxa-test",
                    "user_token": user_token,
                    "bot_token": bot_token
                }
            })
            .to_string(),
        };
        let (status, _, payload) = route(&update, &graph, true, &runtime);
        assert_eq!(status, "200 OK");
        assert!(payload.contains(r#""configured":true"#));
        for secret in [api_key, user_token, bot_token] {
            assert!(!payload.contains(secret));
            assert!(!fs::read_to_string(&graph).unwrap().contains(secret));
        }

        let status_request = HttpRequest {
            method: "GET".into(),
            path: "/api/v1/providers".into(),
            authorization: None,
            body: String::new(),
        };
        let (_, _, payload) = route(&status_request, &graph, true, &runtime);
        assert!(payload.contains(r#""api_key_set":true"#));
        assert!(payload.contains(r#""user_token_set":true"#));
        assert!(payload.contains(r#""bot_token_set":true"#));
        assert_eq!(fs::read_to_string(&graph).unwrap(), original);
        fs::remove_file(graph).unwrap();
    }
}
