//! Local-only Muxiva Graph Studio server with bundled, dependency-free assets.

mod media_dump;
mod node_library;
mod observability;
mod semantic_trace;

use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use muxiva_core::{
    start_registered_runtime_with_context_and_observer, EdgePolicies, FrameObservation,
    GraphRuntime, NotificationBus, ResourceStore, RuntimeObserver, RuntimeOptions,
    RuntimeWaitError, SignalObservation,
};
use muxiva_graph_json::{GraphDiagnostic, GraphDocument, MAX_DOCUMENT_BYTES};
use muxiva_types::{EdgeId, NamespacedName, NodeId};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const INDEX: &str = include_str!("assets/index.html");
const STYLES: &str = include_str!("assets/studio.css");
const BRAND_STYLES: &str = include_str!("assets/brand.css");
const RUNTIME_STYLES: &str = include_str!("assets/runtime.css");
const OBSERVABILITY_STYLES: &str = include_str!("assets/observability.css");
const OBSERVABILITY_HISTORY_STYLES: &str = include_str!("assets/observability-history.css");
const MEDIA_DUMP_STYLES: &str = include_str!("assets/media-dump.css");
const SEMANTIC_TRACE_STYLES: &str = include_str!("assets/semantic-trace.css");
const NODE_LAB_STYLES: &str = include_str!("assets/node-lab.css");
const PROVIDER_HELP_STYLES: &str = include_str!("assets/provider-help.css");
const SCRIPT: &str = include_str!("assets/studio.js");
const LOGO: &[u8] = include_bytes!("assets/muxiva-logo.png");
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct RuntimeSession {
    id: u64,
    run_id: String,
    started_at_unix_ms: u64,
    graph_id: String,
    channel: Option<String>,
    node_ids: Vec<NodeId>,
    edge_ids: Vec<EdgeId>,
    runtime: GraphRuntime,
    started: Instant,
    stop_requested: bool,
    last_observability_log: Mutex<Instant>,
}

struct StudioRuntime {
    next_session: AtomicU64,
    session: Mutex<Option<RuntimeSession>>,
    connections: node_library::ConnectionStore,
    events: Arc<Mutex<VecDeque<serde_json::Value>>>,
    observability: observability::ObservabilityStore,
    media_dumps: Arc<media_dump::MediaDumpStore>,
    semantic_traces: Arc<semantic_trace::SemanticTraceStore>,
}

struct StudioRuntimeObserver {
    media_dumps: Arc<media_dump::MediaDumpStore>,
    semantic_traces: Arc<semantic_trace::SemanticTraceStore>,
}

impl RuntimeObserver for StudioRuntimeObserver {
    fn observe_frame(&self, observation: FrameObservation<'_>) {
        self.media_dumps.observe_frame(observation);
        self.semantic_traces.observe_frame(observation);
    }

    fn observe_signal(&self, observation: SignalObservation<'_>) {
        self.semantic_traces.observe_signal(observation);
    }
}

impl StudioRuntime {
    fn new(graph: &Path) -> Result<Self, String> {
        let observability = observability::ObservabilityStore::new(graph);
        let latest_session_id = observability.latest_session_id();
        Ok(Self {
            next_session: AtomicU64::new(latest_session_id),
            session: Mutex::new(None),
            connections: node_library::ConnectionStore::load(graph)?,
            events: Arc::new(Mutex::new(VecDeque::with_capacity(128))),
            observability,
            media_dumps: Arc::new(media_dump::MediaDumpStore::new(graph)),
            semantic_traces: Arc::new(semantic_trace::SemanticTraceStore::new()),
        })
    }
}

pub fn random_token() -> std::io::Result<String> {
    let mut bytes = [0_u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn serve(listener: TcpListener, graph: PathBuf, token: String) -> std::io::Result<()> {
    let runtime = Arc::new(StudioRuntime::new(&graph).map_err(std::io::Error::other)?);
    let sampler = Arc::downgrade(&runtime);
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(5));
        let Some(runtime) = sampler.upgrade() else {
            break;
        };
        let _ = runtime_snapshot_value(&runtime);
    });
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
            return write_response(
                &mut stream,
                error.status,
                "text/plain",
                error.message,
                false,
            )
        }
    };
    if request.method == "GET" && request.path == "/assets/muxiva-logo.png" {
        return write_response_bytes(&mut stream, "200 OK", "image/png", LOGO, false);
    }
    let authorized = request.authorization.as_deref() == Some(&format!("Bearer {token}"));
    if authorized
        && request.method == "GET"
        && request
            .path
            .starts_with("/api/v1/observability/media-artifacts/")
    {
        let relative = &request.path["/api/v1/observability/media-artifacts/".len()..];
        let mut parts = relative.split('/');
        let run_id = parts.next().unwrap_or_default();
        let artifact_id = parts.next().unwrap_or_default();
        if parts.next().is_none() {
            return match runtime.media_dumps.read_artifact(run_id, artifact_id) {
                Ok((content_type, _, payload)) => {
                    write_response_bytes(&mut stream, "200 OK", &content_type, &payload, false)
                }
                Err(message) => write_response(
                    &mut stream,
                    "404 Not Found",
                    "application/json",
                    &json_message(&message),
                    false,
                ),
            };
        }
    }
    let (status, content_type, payload) = route(&request, graph, authorized, runtime);
    write_response(&mut stream, status, content_type, &payload, false)
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
        ("GET", "/assets/brand.css") => {
            ("200 OK", "text/css; charset=utf-8", BRAND_STYLES.to_owned())
        }
        ("GET", "/assets/runtime.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            RUNTIME_STYLES.to_owned(),
        ),
        ("GET", "/assets/observability.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            OBSERVABILITY_STYLES.to_owned(),
        ),
        ("GET", "/assets/observability-history.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            OBSERVABILITY_HISTORY_STYLES.to_owned(),
        ),
        ("GET", "/assets/media-dump.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            MEDIA_DUMP_STYLES.to_owned(),
        ),
        ("GET", "/assets/semantic-trace.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            SEMANTIC_TRACE_STYLES.to_owned(),
        ),
        ("GET", "/assets/node-lab.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            NODE_LAB_STYLES.to_owned(),
        ),
        ("GET", "/assets/provider-help.css") => (
            "200 OK",
            "text/css; charset=utf-8",
            PROVIDER_HELP_STYLES.to_owned(),
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
            muxiva_graph_json::GRAPH_V1_SCHEMA.to_owned(),
        ),
        ("GET", "/api/v1/registry/nodes") => catalog_response(graph, runtime),
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
        ("GET", "/api/v1/templates") => match project_templates(graph) {
            Ok(templates) => (
                "200 OK",
                "application/json",
                serde_json::Value::Array(templates).to_string(),
            ),
            Err(error) => (
                "500 Internal Server Error",
                "application/json",
                json_message(&format!("failed to load project templates: {error}")),
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
        ("GET", "/api/v1/connections" | "/api/v1/providers") => (
            "200 OK",
            "application/json",
            runtime.connections.status_json().to_string(),
        ),
        ("GET", "/api/v1/official-node-collections" | "/api/v1/provider-catalog") => {
            match node_library::provider_catalog(graph) {
                Ok(providers) => (
                    "200 OK",
                    "application/json",
                    serde_json::to_string(&providers).unwrap_or_else(|_| "[]".into()),
                ),
                Err(error) => (
                    "500 Internal Server Error",
                    "application/json",
                    json_message(&format!(
                        "failed to load official Node collections: {error}"
                    )),
                ),
            }
        }
        ("PUT", "/api/v1/connections" | "/api/v1/providers") => {
            match runtime.connections.update_json(&request.body) {
                Ok(()) => (
                    "200 OK",
                    "application/json",
                    runtime.connections.status_json().to_string(),
                ),
                Err(message) => (
                    "400 Bad Request",
                    "application/json",
                    json_message(&message),
                ),
            }
        }
        ("POST", "/api/v1/graph/validate") => match validate(&request.body, graph, runtime) {
            Ok(_) => ("200 OK", "application/json", "[]".into()),
            Err(errors) => diagnostics_response(errors),
        },
        ("GET", "/api/v1/runtime") => ("200 OK", "application/json", runtime_snapshot(runtime)),
        ("GET", "/metrics" | "/api/v1/observability/prometheus") => {
            let snapshot = runtime_snapshot_value(runtime);
            let current = snapshot["session_id"].is_number().then_some(&snapshot);
            (
                "200 OK",
                "text/plain; version=0.0.4; charset=utf-8",
                observability::prometheus(current),
            )
        }
        ("GET", "/api/v1/observability/history") => (
            "200 OK",
            "application/json",
            runtime.observability.history_index().to_string(),
        ),
        ("GET", "/api/v1/observability/traces") => (
            "200 OK",
            "application/json",
            runtime.semantic_traces.status_json(None).to_string(),
        ),
        ("GET", path) if path.starts_with("/api/v1/observability/traces/") => {
            let run_id = &path["/api/v1/observability/traces/".len()..];
            (
                "200 OK",
                "application/json",
                runtime
                    .semantic_traces
                    .status_json(Some(run_id))
                    .to_string(),
            )
        }
        ("GET", "/api/v1/observability/media") => (
            "200 OK",
            "application/json",
            runtime.media_dumps.status_json(None).to_string(),
        ),
        ("GET", path) if path.starts_with("/api/v1/observability/media/") => {
            let run_id = &path["/api/v1/observability/media/".len()..];
            (
                "200 OK",
                "application/json",
                runtime.media_dumps.status_json(Some(run_id)).to_string(),
            )
        }
        ("PUT", "/api/v1/observability/media") => {
            match serde_json::from_str::<serde_json::Value>(&request.body)
                .ok()
                .and_then(|value| value["enabled"].as_bool())
            {
                Some(enabled) => {
                    runtime.media_dumps.set_enabled(enabled);
                    (
                        "200 OK",
                        "application/json",
                        runtime.media_dumps.status_json(None).to_string(),
                    )
                }
                None => (
                    "400 Bad Request",
                    "application/json",
                    json_message("media dump configuration requires boolean `enabled`"),
                ),
            }
        }
        ("GET", path) if path.starts_with("/api/v1/observability/history/") => {
            let run_id = &path["/api/v1/observability/history/".len()..];
            match runtime.observability.history_session(run_id) {
                Some(value) => ("200 OK", "application/json", value.to_string()),
                None => (
                    "404 Not Found",
                    "application/json",
                    json_message("observability session not found"),
                ),
            }
        }
        ("GET", "/api/v1/runtime/events") => (
            "200 OK",
            "application/json",
            runtime_events(runtime).to_string(),
        ),
        ("POST", "/api/v1/runtime/start") => start_runtime(runtime, &request.body, graph),
        ("POST", "/api/v1/runtime/stop") => stop_runtime(runtime),
        ("PUT", "/api/v1/graph") => match validate(&request.body, graph, runtime) {
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

fn project_templates(graph: &Path) -> std::io::Result<Vec<serde_json::Value>> {
    let root = graph
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".muxiva/templates");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let bytes = fs::read(&path)?;
            if bytes.len() > MAX_DOCUMENT_BYTES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "template exceeds the Graph document size limit",
                ));
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            if !value.get("id").is_some_and(serde_json::Value::is_string)
                || !value.get("name").is_some_and(serde_json::Value::is_string)
                || !value.get("graph").is_some_and(serde_json::Value::is_object)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "template requires string id/name and a graph object",
                ));
            }
            Ok(value)
        })
        .collect()
}

/// Builds the exact runtime Registry for a project Graph and its local Node
/// Library without starting Studio or executing a Node lifecycle callback.
pub fn project_registry(graph: &Path) -> Result<muxiva_core::NodeRegistry, String> {
    let connections = node_library::ConnectionStore::load(graph)?;
    let mut registry = muxiva_graph_json::builtin_registry();
    node_library::register_project_nodes_with_connections(graph, &mut registry, connections)?;
    Ok(registry)
}

/// Returns only fields explicitly marked `client_exposed` by Node manifests.
/// It is used by a separately deployed browser client and never includes model
/// credentials, Agora bot tokens, Graph mutation, or Runtime control APIs.
pub fn project_client_session(graph: &Path) -> Result<serde_json::Value, String> {
    Ok(node_library::ConnectionStore::load(graph)?.client_json())
}

fn studio_project_registry(
    graph: &Path,
    runtime: &StudioRuntime,
) -> Result<muxiva_core::NodeRegistry, String> {
    let mut registry = muxiva_graph_json::builtin_registry();
    node_library::register_project_nodes_with_connections(
        graph,
        &mut registry,
        runtime.connections.clone(),
    )?;
    Ok(registry)
}

fn catalog_response(graph: &Path, runtime: &StudioRuntime) -> (&'static str, &'static str, String) {
    match studio_project_registry(graph, runtime) {
        Ok(registry) => (
            "200 OK",
            "application/json",
            serde_json::to_string(&muxiva_graph_json::node_catalog(&registry))
                .unwrap_or_else(|_| "[]".into()),
        ),
        Err(error) => (
            "400 Bad Request",
            "application/json",
            json_message(&format!("invalid project Node Library: {error}")),
        ),
    }
}

fn validate(
    input: &str,
    graph_path: &Path,
    runtime: &StudioRuntime,
) -> Result<GraphDocument, Vec<GraphDiagnostic>> {
    let registry = studio_project_registry(graph_path, runtime).map_err(|message| {
        vec![GraphDiagnostic {
            code: "MUXIVA-STUDIO-NODE-LIBRARY".into(),
            message,
            pointer: "/.muxiva/nodes".into(),
        }]
    })?;
    muxiva_graph_json::parse(input).and_then(|document| {
        muxiva_graph_json::compile_with_registry(&document, &registry).map(|_| document)
    })
}

fn start_runtime(
    state: &StudioRuntime,
    input: &str,
    graph_path: &Path,
) -> (&'static str, &'static str, String) {
    let document = match muxiva_graph_json::parse(input) {
        Ok(document) => document,
        Err(errors) => return diagnostics_response(errors),
    };
    let used_node_types = document
        .nodes
        .iter()
        .map(|node| node.node_type.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let required_connections = match node_library::list(graph_path) {
        Ok(packages) => packages
            .into_iter()
            .filter(|package| used_node_types.contains(package.manifest.node_type.as_str()))
            .filter_map(|package| package.resolved_connection_id().map(str::to_owned))
            .collect::<std::collections::BTreeSet<_>>(),
        Err(error) => {
            return (
                "400 Bad Request",
                "application/json",
                json_message(&format!("invalid project Node Library: {error}")),
            )
        }
    };
    let missing = state
        .connections
        .missing_required_for(&required_connections);
    if !missing.is_empty() {
        return (
            "412 Precondition Failed",
            "application/json",
            json_message(&format!(
                "Runtime not started. Open Connections and configure: {}",
                missing.join(", ")
            )),
        );
    }
    let registry = match studio_project_registry(graph_path, state) {
        Ok(registry) => registry,
        Err(error) => {
            return (
                "400 Bad Request",
                "application/json",
                json_message(&format!("invalid project Node Library: {error}")),
            )
        }
    };
    let graph = match muxiva_graph_json::compile_with_registry(&document, &registry) {
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
    let notification_bus = match studio_notification_bus(&state.events) {
        Ok(bus) => bus,
        Err(error) => {
            return (
                "500 Internal Server Error",
                "application/json",
                json_message(&format!("failed to start Studio event telemetry: {error}")),
            )
        }
    };
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
    let id = state.next_session.fetch_add(1, Ordering::Relaxed) + 1;
    let started_at_unix_ms = observability::unix_time_ms();
    let run_id = format!("{started_at_unix_ms}-{id}");
    let channel = state
        .connections
        .client_json()
        .as_object()
        .and_then(|connections| {
            connections.values().find_map(|connection| {
                connection
                    .get("channel")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            })
        });
    state.media_dumps.start_session(&run_id);
    state.semantic_traces.start_session(&run_id);
    let observer = Arc::new(StudioRuntimeObserver {
        media_dumps: state.media_dumps.clone(),
        semantic_traces: state.semantic_traces.clone(),
    });
    let runtime = match start_registered_runtime_with_context_and_observer(
        graph,
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
        ResourceStore::new(),
        notification_bus,
        Some(observer),
    ) {
        Ok(runtime) => runtime,
        Err(error) => {
            state.media_dumps.finish_session(&run_id);
            state.semantic_traces.finish_session(&run_id);
            return (
                "500 Internal Server Error",
                "application/json",
                json_message(&format!("failed to start graph runtime: {error}")),
            );
        }
    };
    *session = Some(RuntimeSession {
        id,
        run_id: run_id.clone(),
        started_at_unix_ms,
        graph_id,
        channel,
        node_ids,
        edge_ids,
        runtime,
        started: Instant::now(),
        stop_requested: false,
        last_observability_log: Mutex::new(Instant::now()),
    });
    let mut snapshot = session_snapshot(session.as_ref().expect("installed session"));
    snapshot["media_dump"] = state.media_dumps.status_json(Some(&run_id));
    state.observability.observe(&snapshot);
    ("201 Created", "application/json", snapshot.to_string())
}

fn studio_notification_bus(
    events: &Arc<Mutex<VecDeque<serde_json::Value>>>,
) -> Result<NotificationBus, String> {
    events
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    let bus = NotificationBus::default();
    for topic in [
        "muxiva.voice.speech.started",
        "muxiva.voice.speech.stopped",
        "muxiva.voice.barge_in",
        "muxiva.voice.transcript.preview",
        "muxiva.voice.transcript.delta",
        "muxiva.voice.transcript.completed",
        "muxiva.voice.transcript.failed",
        "muxiva.voice.response.delta",
        "muxiva.voice.response.completed",
        "muxiva.agent.response.started",
        "muxiva.agent.response.delta",
        "muxiva.agent.response.completed",
        "muxiva.agent.response.failed",
        "muxiva.agent.response.cancelled",
        "muxiva.agent.tool.started",
        "muxiva.agent.tool.updated",
        "muxiva.agent.tool.completed",
    ] {
        let queue = events.clone();
        bus.subscribe(
            NamespacedName::new(topic).map_err(|error| error.to_string())?,
            move |event| {
                let data = event.data();
                let value = serde_json::json!({
                    "topic": data.topic().as_str(),
                    "source": data.source().as_str(),
                    "sequence": event.header().sequence_id().get(),
                    "payload": muxiva_graph_json::value_to_json(data.payload()),
                });
                let mut queue = queue.lock().unwrap_or_else(|error| error.into_inner());
                if queue.len() == 128 {
                    queue.pop_front();
                }
                queue.push_back(value);
                Ok(())
            },
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(bus)
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
    state.observability.observe(&snapshot);
    snapshot["accepted"] = accepted.into();
    ("200 OK", "application/json", snapshot.to_string())
}

fn runtime_snapshot(state: &StudioRuntime) -> String {
    runtime_snapshot_value(state).to_string()
}

fn runtime_snapshot_value(state: &StudioRuntime) -> serde_json::Value {
    let session = state
        .session
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut snapshot = session.as_ref().map_or_else(
        || serde_json::json!({"status": "idle", "session_id": null}),
        session_snapshot,
    );
    if let Some(session) = session.as_ref() {
        if matches!(
            snapshot["status"].as_str(),
            Some("completed" | "aborted" | "stopped")
        ) {
            state.media_dumps.finish_session(&session.run_id);
            state.semantic_traces.finish_session(&session.run_id);
        }
        snapshot["media_dump"] = state.media_dumps.status_json(Some(&session.run_id));
    } else {
        snapshot["media_dump"] = state.media_dumps.status_json(None);
    }
    state.observability.observe(&snapshot);
    snapshot
}

fn runtime_events(state: &StudioRuntime) -> serde_json::Value {
    serde_json::Value::Array(
        state
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .cloned()
            .collect(),
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
                "process_duration_ns": metrics.process_duration_ns(),
                "max_process_duration_ns": metrics.max_process_duration_ns(),
                "custom_metrics": metrics.custom_metrics().iter().map(|metric| serde_json::json!({
                    "name": metric.name(),
                    "kind": format!("{:?}", metric.kind()).to_lowercase(),
                    "value": metric.value(),
                })).collect::<Vec<_>>(),
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
                "payload_bytes_total": metrics.payload_bytes_total(),
                "audio_duration_ns_total": metrics.audio_duration_ns_total(),
                "latest_error_reason": metrics.latest_error_reason(),
            })
        })
        .collect::<Vec<_>>();
    log_observability_snapshot(session, status, &nodes, &edges);
    serde_json::json!({
        "session_id": session.id,
        "run_id": session.run_id,
        "started_at_unix_ms": session.started_at_unix_ms,
        "graph_id": session.graph_id,
        "channel": session.channel,
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

fn log_observability_snapshot(
    session: &RuntimeSession,
    status: &str,
    nodes: &[serde_json::Value],
    edges: &[serde_json::Value],
) {
    if !matches!(status, "running" | "stopping") {
        return;
    }
    let mut last = session
        .last_observability_log
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if last.elapsed() < Duration::from_secs(5) {
        return;
    }
    *last = Instant::now();
    let queued = edges
        .iter()
        .filter_map(|edge| edge["queue_len"].as_u64())
        .sum::<u64>();
    let drops = edges
        .iter()
        .filter_map(|edge| edge["drop_total"].as_u64())
        .sum::<u64>();
    let mut bottlenecks = 0_u64;
    for edge in edges {
        let id = edge["edge_id"].as_str().unwrap_or("unknown");
        let len = edge["queue_len"].as_u64().unwrap_or(0);
        let capacity = edge["queue_capacity"].as_u64().unwrap_or(0);
        let age_ms = edge["oldest_frame_age_ns"].as_u64().unwrap_or(0) / 1_000_000;
        let edge_drops = edge["drop_total"].as_u64().unwrap_or(0);
        let fullness = if capacity == 0 {
            0.0
        } else {
            len as f64 / capacity as f64
        };
        let level = if edge_drops > 0 || fullness >= 0.8 || age_ms >= 1_000 {
            Some("CRITICAL")
        } else if fullness >= 0.4 || age_ms >= 200 {
            Some("WARN")
        } else {
            None
        };
        if let Some(level) = level {
            bottlenecks += 1;
            eprintln!(
                "[MUXIVA][OBSERVE][EDGE][{level}] edge={id} queue={len}/{capacity} oldest_ms={age_ms} drops={edge_drops}"
            );
        }
    }
    for node in nodes {
        let id = node["node_id"].as_str().unwrap_or("unknown");
        let process_total = node["process_total"].as_u64().unwrap_or(0);
        let average_ms = if process_total == 0 {
            0.0
        } else {
            node["process_duration_ns"].as_u64().unwrap_or(0) as f64
                / process_total as f64
                / 1_000_000.0
        };
        let queue_ms = node["custom_metrics"]
            .as_array()
            .and_then(|metrics| {
                metrics
                    .iter()
                    .find(|metric| metric["name"].as_str() == Some("ingress.queue_duration_ms"))
            })
            .and_then(|metric| metric["value"].as_u64())
            .unwrap_or(0);
        let level = if queue_ms >= 1_000 || average_ms >= 50.0 {
            Some("CRITICAL")
        } else if queue_ms >= 200 || average_ms >= 10.0 {
            Some("WARN")
        } else {
            None
        };
        if let Some(level) = level {
            bottlenecks += 1;
            eprintln!(
                "[MUXIVA][OBSERVE][NODE][{level}] node={id} avg_process_ms={average_ms:.2} ingress_queue_ms={queue_ms}"
            );
        }
    }
    eprintln!(
        "[MUXIVA][OBSERVE][SUMMARY] session={} nodes={} edges={} queued={} drops={} bottlenecks={} dashboard=Studio/Observe",
        session.id,
        nodes.len(),
        edges.len(),
        queued,
        drops,
        bottlenecks
    );
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
    project_asset: bool,
) -> std::io::Result<()> {
    write_response_bytes(
        stream,
        status,
        content_type,
        payload.as_bytes(),
        project_asset,
    )
}

fn write_response_bytes(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    payload: &[u8],
    project_asset: bool,
) -> std::io::Result<()> {
    let policy = if project_asset {
        "default-src 'none'; script-src 'self' https:; style-src 'self'; connect-src 'self' https: wss:; media-src blob:; worker-src blob:; img-src 'self' data:; base-uri 'none'; frame-ancestors 'none'"
    } else {
        "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; media-src 'self' blob:; base-uri 'none'; frame-ancestors 'none'"
    };
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nContent-Security-Policy: {policy}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(headers.as_bytes())?;
    stream.write_all(payload)
}

#[cfg(test)]
mod tests {
    use super::{
        handle_connection, project_templates, route, validate, HttpRequest, StudioRuntime, LOGO,
        SCRIPT,
    };
    use std::{
        fs,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn installed_voice_templates_compile_against_the_real_project_registry() {
        let Ok(graph) = std::env::var("MUXIVA_VOICE_FIXTURE_GRAPH") else {
            return;
        };
        let graph = PathBuf::from(graph);
        let runtime = StudioRuntime::new(&graph).unwrap();
        let templates = project_templates(&graph).unwrap();
        assert_eq!(templates.len(), 2);
        for template in templates {
            let graph_json = template["graph"].to_string();
            if let Err(diagnostics) = validate(&graph_json, &graph, &runtime) {
                panic!("template {} failed: {diagnostics:?}", template["id"]);
            }
        }
    }

    fn graph_path() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "muxiva-studio-contract-{}-{}",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("graph.json");
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
            let runtime = StudioRuntime::new(&graph).unwrap();
            handle_connection(stream, &graph, &token, &runtime).unwrap();
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
        assert!(response.contains("/assets/brand.css"));
        assert!(response.contains("/assets/observability.css"));
        assert!(response.contains("/assets/observability-history.css"));
        assert!(response.contains("/assets/media-dump.css"));
        assert!(response.contains("/assets/semantic-trace.css"));
        assert!(response.contains("/assets/muxiva-logo.png"));
        assert!(LOGO.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(response.contains("Node media dumps"));
        assert!(response.contains("Semantic trace"));
        assert!(response.contains("Select one runtime session"));
        assert!(response.contains("observe-session-select"));
        assert!(!response.contains("<h2>Session history</h2>"));
        assert!(response.contains("◎ Observe"));
        assert!(!response.contains("<script>"));
    }

    #[test]
    fn runtime_polling_updates_canvas_metrics_without_rebuilding_the_svg_graph() {
        assert!(SCRIPT.contains("function updateCanvasRuntime()"));
        assert!(SCRIPT.contains("renderRuntime(); updateCanvasRuntime()"));
        assert!(!SCRIPT.contains("renderRuntime(); renderCanvas()"));
        assert!(SCRIPT.contains("data-runtime-metrics"));
    }

    #[test]
    fn media_dump_is_opt_in_and_can_be_toggled_through_the_authenticated_api() {
        let graph = graph_path();
        let runtime = StudioRuntime::new(&graph).unwrap();
        let read = HttpRequest {
            method: "GET".into(),
            path: "/api/v1/observability/media".into(),
            authorization: None,
            body: String::new(),
        };
        let (status, _, payload) = route(&read, &graph, true, &runtime);
        assert_eq!(status, "200 OK");
        assert!(
            !serde_json::from_str::<serde_json::Value>(&payload).unwrap()["enabled"]
                .as_bool()
                .unwrap()
        );

        let enable = HttpRequest {
            method: "PUT".into(),
            path: "/api/v1/observability/media".into(),
            authorization: None,
            body: r#"{"enabled":true}"#.into(),
        };
        let (status, _, payload) = route(&enable, &graph, true, &runtime);
        assert_eq!(status, "200 OK");
        assert!(
            serde_json::from_str::<serde_json::Value>(&payload).unwrap()["enabled"]
                .as_bool()
                .unwrap()
        );
        fs::remove_file(graph).unwrap();
    }

    #[test]
    fn graph_api_rejects_missing_and_forged_bearer_tokens() {
        for path in [
            "/api/v1/graph",
            "/api/v1/runtime",
            "/api/v1/observability/history",
            "/api/v1/observability/traces",
            "/metrics",
        ] {
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
        assert!(response.contains("category"));
        assert!(response.contains("capability"));
        assert!(response.contains("text.source"));
    }

    #[test]
    fn authorized_validation_and_atomic_save_share_graph_v1_contract() {
        let graph = graph_path();
        let invalid = r#"{"version":"muxiva.graph/v1","graph_id":"broken","nodes":[],"edges":[],"unexpected":true}"#;
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
        assert!(validation_response.contains("MUXIVA-GRAPH-JSON"));

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
        let runtime = StudioRuntime::new(&graph).unwrap();
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
                assert!(value["edges"][0]["payload_bytes_total"].as_u64().unwrap() > 0);
                assert_eq!(value["nodes"][0]["prepare_total"], 1);
                assert!(value["nodes"][0]["process_duration_ns"].is_number());
                assert!(value["nodes"][0]["custom_metrics"].is_array());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "runtime did not complete"
            );
            thread::yield_now();
        }
        let metrics_request = HttpRequest {
            method: "GET".into(),
            path: "/metrics".into(),
            authorization: None,
            body: String::new(),
        };
        let (status, content_type, payload) = route(&metrics_request, &graph, true, &runtime);
        assert_eq!(status, "200 OK");
        assert!(content_type.starts_with("text/plain"));
        assert!(payload.contains("muxiva_node_process_total"));
        assert!(payload.contains("muxiva_edge_queue_length"));

        let history_request = HttpRequest {
            method: "GET".into(),
            path: "/api/v1/observability/history".into(),
            authorization: None,
            body: String::new(),
        };
        let (_, _, payload) = route(&history_request, &graph, true, &runtime);
        let history: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(history["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(history["sessions"][0]["status"], "completed");
        let run_id = history["sessions"][0]["run_id"].as_str().unwrap();
        let details_request = HttpRequest {
            method: "GET".into(),
            path: format!("/api/v1/observability/history/{run_id}"),
            authorization: None,
            body: String::new(),
        };
        let (_, _, payload) = route(&details_request, &graph, true, &runtime);
        let details: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert!(!details["samples"].as_array().unwrap().is_empty());
        let trace_request = HttpRequest {
            method: "GET".into(),
            path: format!("/api/v1/observability/traces/{run_id}"),
            authorization: None,
            body: String::new(),
        };
        let (_, _, payload) = route(&trace_request, &graph, true, &runtime);
        let trace: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(trace["session"]["status"], "completed");
        assert_eq!(trace["session"]["turns"].as_array().unwrap().len(), 1);
        assert_eq!(trace["session"]["entries"], 4);
        assert!(trace["session"]["turns"][0]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["kind"] == "text"));
        fs::remove_file(graph).unwrap();
    }

    #[test]
    fn provider_configuration_persists_secrets_only_in_gitignored_dotenv() {
        let graph = graph_path();
        let original = fs::read_to_string(&graph).unwrap();
        let package_dir = graph
            .parent()
            .unwrap()
            .join(".muxiva/nodes/connection_test");
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(
            package_dir.join("muxiva.node.json"),
            r#"{"format":"muxiva.node/v1","package_id":"connection_test","display_name":"Connection Test","node_type":"test.connection","language":"python","factory_version":"1.0.0","kind":"transform","entrypoint":"node:Node","ports":[{"name":"text_in","direction":"input","frame_type":"text"},{"name":"text_out","direction":"output","frame_type":"text"}],"config_schema":{"type":"object"},"connection":{"id":"test_provider","display_name":"Test Provider","description":"Generic manifest connection","fields":[{"name":"api_key","label":"API Key","environment":"TEST_PROVIDER_API_KEY","secret":true,"required":true,"default":""},{"name":"endpoint","label":"Endpoint","environment":"TEST_PROVIDER_ENDPOINT","secret":false,"required":true,"client_exposed":true,"default":"https://example.test"}]}}"#,
        )
        .unwrap();
        fs::write(package_dir.join("node.py"), "class Node: pass\n").unwrap();
        let runtime = StudioRuntime::new(&graph).unwrap();
        let api_key = "private-test-key";
        let update = HttpRequest {
            method: "PUT".into(),
            path: "/api/v1/providers".into(),
            authorization: None,
            body: serde_json::json!({
                "connections": {
                    "test_provider": {
                        "api_key": api_key,
                        "endpoint": "https://custom.example"
                    }
                }
            })
            .to_string(),
        };
        let (status, _, payload) = route(&update, &graph, true, &runtime);
        assert_eq!(status, "200 OK");
        assert!(payload.contains(r#""configured":true"#));
        assert!(!payload.contains(api_key));
        assert!(!fs::read_to_string(&graph).unwrap().contains(api_key));
        let dotenv = graph.parent().unwrap().join(".env");
        let dotenv_text = fs::read_to_string(&dotenv).unwrap();
        assert!(dotenv_text.contains("TEST_PROVIDER_API_KEY="));
        assert!(dotenv_text.contains(api_key));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&dotenv).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let status_request = HttpRequest {
            method: "GET".into(),
            path: "/api/v1/providers".into(),
            authorization: None,
            body: String::new(),
        };
        let (_, _, payload) = route(&status_request, &graph, true, &runtime);
        assert!(payload.contains(r#""name":"api_key"#));
        assert!(payload.contains(r#""set":true"#));
        assert!(payload.contains("https://custom.example"));
        let client_payload = crate::project_client_session(&graph).unwrap().to_string();
        assert!(client_payload.contains("https://custom.example"));
        assert!(!client_payload.contains(api_key));
        assert!(!client_payload.contains("api_key"));
        assert_eq!(fs::read_to_string(&graph).unwrap(), original);
        fs::remove_dir_all(graph.parent().unwrap()).unwrap();
    }

    #[test]
    fn runtime_preflight_rejects_missing_credentials_before_node_creation() {
        let graph = graph_path();
        let package_dir = graph.parent().unwrap().join(".muxiva/nodes/requires_key");
        fs::create_dir_all(&package_dir).unwrap();
        fs::write(
            package_dir.join("muxiva.node.json"),
            r#"{"format":"muxiva.node/v1","package_id":"requires_key","display_name":"Requires Key","node_type":"test.requires_key","language":"python","factory_version":"1.0.0","kind":"source","entrypoint":"node:Node","ports":[],"config_schema":{"type":"object"},"connection":{"id":"test_service","display_name":"Test Service","description":"Test-only connection","fields":[{"name":"api_key","label":"API Key","environment":"MUXIVA_TEST_REQUIRED_KEY","secret":true,"required":true,"default":""}]}}"#,
        )
        .unwrap();
        fs::write(package_dir.join("node.py"), "class Node: pass\n").unwrap();
        let runtime = StudioRuntime::new(&graph).unwrap();
        let request = HttpRequest {
            method: "POST".into(),
            path: "/api/v1/runtime/start".into(),
            authorization: None,
            body: r#"{"version":"muxiva.graph/v1","graph_id":"preflight","nodes":[{"id":"guarded","node_type":"test.requires_key","language":"python","factory_version":"1.0.0","node_config":{}}],"edges":[]}"#.into(),
        };
        let (status, _, payload) = route(&request, &graph, true, &runtime);
        assert_eq!(status, "412 Precondition Failed");
        assert!(payload.contains("Runtime not started"));
        assert!(payload.contains("MUXIVA_TEST_REQUIRED_KEY"));
        fs::remove_dir_all(graph.parent().unwrap()).unwrap();
    }
}
