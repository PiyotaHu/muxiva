use clap::{Parser, Subcommand, ValueEnum};
use muxiva_core::{
    materialize_registered_nodes, start_registered_runtime, ConcurrentRuntime, EdgePolicies,
    NodeRegistry, NotificationBus, RuntimeOptions, RuntimeWaitError,
};
use muxiva_types::NamespacedName;
use std::{
    env, fs,
    io::IsTerminal,
    net::{IpAddr, TcpListener},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

mod bootstrap_server;
mod doctor;

const DEFAULT_RUN_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_CLIENT_API_PORT: u16 = 8080;
const MAX_CLI_TIMEOUT_MS: u64 = 60 * 60 * 1_000;

const STARTER: &str = r#"{
  "version": "muxiva.graph/v1",
  "graph_id": "text-uppercase",
  "nodes": [
    {"id":"source","node_type":"builtin.text_source","language":"rust","factory_version":"1.0.0","node_config":{"text":"hello"}},
    {"id":"upper","node_type":"builtin.uppercase","language":"rust","factory_version":"1.0.0","node_config":{}},
    {"id":"sink","node_type":"builtin.stdout_text_sink","language":"rust","factory_version":"1.0.0","node_config":{}}
  ],
  "edges": [
    {"id":"source-upper","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"upper","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":32,"overflow":"block"}},
    {"id":"upper-sink","from":{"node_id":"upper","port":"text_out"},"to":{"node_id":"sink","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":32,"overflow":"block"}}
  ]
}"#;
const VOICE_DEMO: &str = include_str!("../../../examples/graphs/mock-realtime-voice.v1.json");
const FLAGSHIP_GUIDE: &str = "https://piyotahu.github.io/muxiva/voice-demo/";
const PROJECT_README: &str = r#"# Muxiva Agent

This project contains a typed Muxiva Graph and project-owned Node packages.

```sh
muxiva validate .
muxiva run .
muxiva serve .
muxiva studio . # optional visual editor
```

Build a real voice assistant with the flagship guide:
https://piyotahu.github.io/muxiva/voice-demo/
"#;
const PROJECT_GITIGNORE: &str =
    ".env\n.muxiva/native/\n.muxiva/venv/\n.muxiva/npm-cache/\n.muxiva/observability/\n";

#[derive(Parser)]
#[command(
    name = "muxiva",
    version,
    about = "Build and run real-time multimodal agent graphs",
    long_about = "Muxiva is a real-time multimodal Agent Runtime for typed, concurrent graphs.\n\nUse serve for a long-running headless Graph, run for a finite Graph, and Studio only when you want the visual editor.",
    after_help = "Start here:\n  muxiva serve graph.json   Run a long-lived Graph without Studio\n  muxiva run graph.json     Execute a finite Graph to completion\n  muxiva studio graph.json  Open the optional visual editor\n  muxiva doctor --voice     Check flagship voice-demo prerequisites\n\nReal voice guide: https://piyotahu.github.io/muxiva/voice-demo/"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open local visual Studio; auto-discover or create a workspace.
    Studio {
        /// Project directory or Graph JSON. Omit to discover the current workspace.
        graph: Option<PathBuf>,
        /// Bind to this exact TCP port; the default selects a free local port.
        #[arg(long)]
        port: Option<u16>,
        /// Bind address. Keep the loopback default for local development.
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        /// Print the URL without opening the default browser.
        #[arg(long)]
        no_open: bool,
    },
    /// Create a Muxiva project with graph.json and project Node directories.
    Init {
        /// New project directory. A .json path creates one Graph for compatibility.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Validate a project or Graph without executing any Node.
    Validate {
        /// Project directory or Graph JSON.
        graph: PathBuf,
    },
    /// Execute a project or Graph with the concurrent Runtime.
    Run {
        /// Project directory or Graph JSON.
        graph: PathBuf,
        /// Maximum wall-clock time to wait for graph completion.
        #[arg(long, default_value_t = DEFAULT_RUN_TIMEOUT_MS)]
        timeout_ms: u64,
        /// Bounded cleanup wait after a run timeout.
        #[arg(long, default_value_t = DEFAULT_SHUTDOWN_TIMEOUT_MS)]
        shutdown_timeout_ms: u64,
    },
    /// Run a long-lived Graph headlessly with a minimal browser bootstrap API.
    Serve {
        /// Project directory or Graph JSON.
        graph: PathBuf,
        /// Client API bind address. Use 0.0.0.0 only behind a firewall or reverse proxy.
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        /// Client API TCP port.
        #[arg(long, default_value_t = DEFAULT_CLIENT_API_PORT)]
        port: u16,
        /// Browser origin allowed to call the client API; repeat for multiple frontends.
        #[arg(long = "allow-origin")]
        allowed_origins: Vec<String>,
        /// Bounded cleanup wait after SIGINT/SIGTERM.
        #[arg(long, default_value_t = DEFAULT_SHUTDOWN_TIMEOUT_MS)]
        shutdown_timeout_ms: u64,
    },
    /// Check local tools, project discovery, and optional voice-demo readiness.
    Doctor {
        /// Also inspect Qwen + Agora flagship voice-demo prerequisites.
        #[arg(long)]
        voice: bool,
        /// Return a failure when a recommended prerequisite is missing.
        #[arg(long)]
        strict: bool,
    },
    /// Run synthetic, network-free Runtime fixtures for engineering checks.
    Simulate {
        /// Fixture: voice exercises fork/join control flow; text is a minimal smoke test.
        #[arg(long, value_enum, default_value_t = SimulationScenario::Voice)]
        scenario: SimulationScenario,
        /// Number of scripted voice turns (1-100).
        #[arg(long, default_value_t = 4)]
        turns: u16,
        /// Delay between source ticks in the synthetic voice fixture.
        #[arg(long, default_value_t = 650)]
        interval_ms: u64,
    },
    /// Deprecated alias for `simulate`; hidden because it is not a product demo.
    #[command(hide = true)]
    Demo {
        #[arg(long, value_enum, default_value_t = SimulationScenario::Voice)]
        scenario: SimulationScenario,
        #[arg(long, default_value_t = 4)]
        turns: u16,
        #[arg(long, default_value_t = 650)]
        interval_ms: u64,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum SimulationScenario {
    Voice,
    Text,
}

fn load(path: &Path, registry: &NodeRegistry) -> Result<muxiva_core::GraphDefinition, String> {
    let data = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    load_source(&data, registry)
}

fn load_source(
    data: &str,
    registry: &NodeRegistry,
) -> Result<muxiva_core::GraphDefinition, String> {
    let document = muxiva_graph_json::parse(data).map_err(render)?;
    muxiva_graph_json::compile_with_registry(&document, registry).map_err(render)
}

fn resolve_graph_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Ok(path.to_owned());
    }
    for name in ["graph.json", "muxiva.graph.json"] {
        let candidate = path.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "project {} contains neither graph.json nor muxiva.graph.json",
        path.display()
    ))
}

fn validate(path: &Path) -> Result<PathBuf, String> {
    let graph = resolve_graph_path(path)?;
    let registry = muxiva_studio::project_registry(&graph)?;
    load(&graph, &registry)?;
    Ok(graph)
}

fn render(errors: Vec<muxiva_graph_json::GraphDiagnostic>) -> String {
    errors
        .into_iter()
        .map(|diagnostic| diagnostic.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn init(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        return init_graph(path);
    }
    if path.exists() && !path.is_dir() {
        return Err(format!("{} exists and is not a directory", path.display()));
    }
    let created_root = !path.exists();
    let graph = path.join("graph.json");
    if graph.exists() {
        return Err(format!("refusing to overwrite {}", graph.display()));
    }
    fs::create_dir_all(path.join(".muxiva/nodes")).map_err(|error| error.to_string())?;
    fs::create_dir_all(path.join(".muxiva/templates")).map_err(|error| error.to_string())?;
    fs::write(&graph, STARTER).map_err(|error| error.to_string())?;
    if created_root {
        fs::write(path.join("README.md"), PROJECT_README).map_err(|error| error.to_string())?;
        fs::write(path.join(".gitignore"), PROJECT_GITIGNORE).map_err(|error| error.to_string())?;
    }
    println!(
        "[MUXIVA][INFO][project.created] root={} graph={} credentials=external",
        path.display(),
        graph.display()
    );
    println!("[MUXIVA][NEXT] muxiva validate {}", path.display());
    Ok(())
}

fn init_graph(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("refusing to overwrite {}", path.display()));
    }
    fs::write(path, STARTER).map_err(|error| error.to_string())?;
    println!(
        "[MUXIVA][INFO][graph.created] path={} credentials=external",
        path.display()
    );
    Ok(())
}

fn discover_studio_graph(current: &Path) -> Option<PathBuf> {
    let current_project = current.join("graph.json");
    if current.join(".muxiva").is_dir() && current_project.is_file() {
        return Some(current_project);
    }
    let standalone = current.join("muxiva.graph.json");
    if standalone.is_file() {
        return Some(standalone);
    }
    for ancestor in current.ancestors() {
        let flagship = ancestor.join("examples/voice-agent/graph.json");
        if flagship.is_file() {
            return Some(flagship);
        }
    }
    None
}

fn studio(
    graph: Option<PathBuf>,
    port: Option<u16>,
    host: IpAddr,
    no_open: bool,
) -> Result<(), String> {
    let graph = match graph {
        Some(path) => resolve_graph_path(&path)?,
        None => {
            let current = env::current_dir().map_err(|error| error.to_string())?;
            if let Some(path) = discover_studio_graph(&current) {
                println!(
                    "[MUXIVA][INFO][studio.workspace] mode=discovered graph={}",
                    path.display()
                );
                path
            } else {
                let path = current.join("muxiva.graph.json");
                init_graph(&path)?;
                println!(
                    "[MUXIVA][INFO][studio.workspace] mode=created graph={}",
                    path.display()
                );
                path
            }
        }
    };
    if !graph.is_file() {
        return Err(format!("Studio graph does not exist: {}", graph.display()));
    }
    let graph = fs::canonicalize(&graph)
        .map_err(|error| format!("cannot resolve {}: {error}", graph.display()))?;
    if !host.is_loopback() {
        eprintln!("[MUXIVA][WARN][studio.non-loopback] access token protections are required");
    }
    let requested = port.unwrap_or(0);
    let listener = TcpListener::bind((host, requested))
        .map_err(|error| format!("cannot bind {host}:{requested}: {error}"))?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let token = studio_access_token()?;
    let url = format!("http://{address}/#{token}");
    println!("[MUXIVA][INFO][studio.ready] url={url}");
    println!("[MUXIVA][INFO][studio.graph] path={}", graph.display());
    println!(
        "[MUXIVA][INFO][studio.metrics] endpoint=http://{address}/metrics auth=bearer history=.muxiva/observability/history.jsonl"
    );
    if !no_open {
        if let Err(error) = open_browser(&url) {
            eprintln!("[MUXIVA][WARN][studio.browser-open] {error}");
        }
    }
    muxiva_studio::serve(listener, graph, token).map_err(|error| error.to_string())
}

fn studio_access_token() -> Result<String, String> {
    let token = match env::var("MUXIVA_STUDIO_ACCESS_TOKEN") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return muxiva_studio::random_token().map_err(|error| error.to_string()),
    };
    validate_access_token("MUXIVA_STUDIO_ACCESS_TOKEN", &token)?;
    Ok(token)
}

fn run(graph_path: &Path, timeout_ms: u64, shutdown_timeout_ms: u64) -> Result<(), String> {
    print_logo();
    let timeout = cli_timeout("timeout-ms", timeout_ms)?;
    let shutdown_timeout = cli_timeout("shutdown-timeout-ms", shutdown_timeout_ms)?;
    let graph_path = resolve_graph_path(graph_path)?;
    let registry = muxiva_studio::project_registry(&graph_path)?;
    let graph = load(&graph_path, &registry)?;
    preflight_project_connections(&graph_path, &graph)?;
    run_graph(
        graph,
        &registry,
        timeout,
        shutdown_timeout,
        timeout_ms,
        None,
    )
}

fn serve_graph(
    graph_path: &Path,
    host: IpAddr,
    port: u16,
    mut allowed_origins: Vec<String>,
    shutdown_timeout_ms: u64,
) -> Result<(), String> {
    let shutdown_timeout = cli_timeout("shutdown-timeout-ms", shutdown_timeout_ms)?;
    let graph_path = resolve_graph_path(graph_path)?;
    let graph_path = fs::canonicalize(&graph_path)
        .map_err(|error| format!("cannot resolve {}: {error}", graph_path.display()))?;
    let registry = muxiva_studio::project_registry(&graph_path)?;
    let graph = load(&graph_path, &registry)?;
    preflight_project_connections(&graph_path, &graph)?;
    let graph_id = graph.graph_id().as_str().to_owned();
    let client_session = muxiva_studio::project_client_session(&graph_path)?;
    if allowed_origins.is_empty() {
        allowed_origins.extend([
            "http://127.0.0.1:4173".to_owned(),
            "http://localhost:4173".to_owned(),
        ]);
    }
    validate_origins(&allowed_origins)?;
    let access_token = env::var("MUXIVA_CLIENT_API_TOKEN")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| project_env_value(&graph_path, "MUXIVA_CLIENT_API_TOKEN"));
    if !host.is_loopback() && access_token.is_none() {
        return Err(
            "non-loopback client API requires MUXIVA_CLIENT_API_TOKEN; set a random 32+ character value in the server environment"
                .into(),
        );
    }
    if let Some(token) = access_token.as_deref() {
        validate_access_token("MUXIVA_CLIENT_API_TOKEN", token)?;
    }
    let client_api_requires_auth = access_token.is_some();
    let stop_requested = Arc::new(AtomicBool::new(false));
    let signal_stop = Arc::clone(&stop_requested);
    ctrlc::set_handler(move || signal_stop.store(true, Ordering::Release))
        .map_err(|error| format!("cannot install shutdown signal handler: {error}"))?;
    let listener = TcpListener::bind((host, port))
        .map_err(|error| format!("cannot bind client API at {host}:{port}: {error}"))?;

    let node_total = graph.nodes().len();
    let edge_total = graph.edges().len();
    println!("[MUXIVA][INFO][graph.loaded] id={graph_id} nodes={node_total} edges={edge_total}");
    println!("[MUXIVA][GRAPH] human-readable DSL");
    print!("{}", graph.render_human_dsl());
    let runtime = start_registered_runtime(
        graph,
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .map_err(|error| format!("cannot start graph `{graph_id}`: {error}"))?;
    let mut client_api = match bootstrap_server::BootstrapServer::start(
        listener,
        graph_id.clone(),
        client_session,
        allowed_origins.clone(),
        access_token,
    ) {
        Ok(server) => server,
        Err(error) => {
            runtime.stop();
            let _ = runtime.wait(shutdown_timeout);
            return Err(error);
        }
    };
    let address = client_api.address();
    println!("[MUXIVA][INFO][runtime.started] mode=headless graph={graph_id}");
    println!(
        "[MUXIVA][INFO][client-api.ready] base_url=http://{address} auth={} cors={}",
        if client_api_requires_auth {
            "bearer"
        } else {
            "none-loopback-only"
        },
        allowed_origins.join(",")
    );
    println!("[MUXIVA][INFO][client-api.health] url=http://{address}/healthz");
    println!(
        "[MUXIVA][NEXT] Start the independent Voice Room, then set Backend URL to http://{address}"
    );
    println!("[MUXIVA][INFO][runtime.control] stop=Ctrl-C studio=not-required");

    loop {
        match runtime.wait(Duration::from_millis(250)) {
            Ok(summary) => {
                client_api.stop();
                println!(
                    "[MUXIVA][INFO][runtime.completed] status=success workers={}",
                    summary.worker_total()
                );
                return Ok(());
            }
            Err(RuntimeWaitError::Aborted(reason)) => {
                client_api.stop();
                return Err(format!(
                    "graph `{graph_id}` aborted: code={} node={} message={}",
                    reason.root().code(),
                    reason
                        .node_id()
                        .map(|node_id| node_id.as_str())
                        .unwrap_or("<runtime>"),
                    reason.root().message()
                ));
            }
            Err(RuntimeWaitError::Timeout(_)) if stop_requested.load(Ordering::Acquire) => {
                println!("[MUXIVA][INFO][runtime.stopping] reason=signal");
                runtime.stop();
                client_api.stop();
                return match runtime.wait(shutdown_timeout) {
                    Err(RuntimeWaitError::Timeout(diagnostics)) => Err(format!(
                        "graph `{graph_id}` did not stop within {shutdown_timeout_ms} ms; active nodes [{}]",
                        diagnostics
                            .active_nodes()
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    )),
                    _ => {
                        println!("[MUXIVA][INFO][runtime.stopped] status=success");
                        Ok(())
                    }
                };
            }
            Err(RuntimeWaitError::Timeout(_)) => {}
        }
    }
}

fn preflight_project_connections(
    graph_path: &Path,
    graph: &muxiva_core::GraphDefinition,
) -> Result<(), String> {
    let missing = muxiva_studio::project_missing_required_connections(graph_path, graph)?;
    if missing.is_empty() {
        return Ok(());
    }
    let project = graph_path.parent().unwrap_or_else(|| Path::new("."));
    let env_path = project.join(".env");
    let example_path = project.join(".env.example");
    Err(format!(
        "required project credentials are missing:\n  - {}\n[MUXIVA][CONFIG] expected={}\n[MUXIVA][NEXT] copy {} to {} and fill the missing values once; Studio is optional",
        missing.join("\n  - "),
        env_path.display(),
        example_path.display(),
        env_path.display(),
    ))
}

fn project_env_value(graph: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(graph.parent()?.join(".env")).ok()?;
    content.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (candidate, value) = line.split_once('=')?;
        (candidate.trim() == key)
            .then(|| value.trim().trim_matches(['\'', '"']).to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn validate_origins(origins: &[String]) -> Result<(), String> {
    for origin in origins {
        if origin == "*" {
            return Err(
                "--allow-origin '*' is not accepted; list each trusted frontend origin".into(),
            );
        }
        let authority = origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"));
        if authority.is_none_or(|authority| authority.is_empty() || authority.contains('/'))
            || origin.contains(['\r', '\n'])
            || origin.ends_with('/')
        {
            return Err(format!(
                "invalid --allow-origin `{origin}`; use an exact origin such as http://127.0.0.1:4173"
            ));
        }
    }
    Ok(())
}

fn validate_access_token(name: &str, token: &str) -> Result<(), String> {
    let valid = (32..=256).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{name} must be 32-256 ASCII letters, digits, '.', '_' or '-'"
        ))
    }
}

fn simulate(scenario: SimulationScenario, turns: u16, interval_ms: u64) -> Result<(), String> {
    if !(1..=100).contains(&turns) {
        return Err("--turns must be between 1 and 100".into());
    }
    if interval_ms > 10_000 {
        return Err("--interval-ms must be between 0 and 10000".into());
    }
    let registry = muxiva_graph_json::builtin_registry();
    let (name, source) = match scenario {
        SimulationScenario::Voice => {
            let mut document: serde_json::Value =
                serde_json::from_str(VOICE_DEMO).map_err(|error| error.to_string())?;
            let microphone = document["nodes"]
                .as_array_mut()
                .and_then(|nodes| nodes.iter_mut().find(|node| node["id"] == "microphone"))
                .ok_or("voice demo microphone is missing")?;
            microphone["node_config"] =
                serde_json::json!({"turns": turns, "interval_ms": interval_ms});
            (
                "realtime-voice-agent",
                serde_json::to_string(&document).map_err(|error| error.to_string())?,
            )
        }
        SimulationScenario::Text => ("text-uppercase", STARTER.to_owned()),
    };
    let graph = load_source(&source, &registry)?;
    println!("[MUXIVA][INFO][simulation.started] fixture={name}");
    if matches!(scenario, SimulationScenario::Voice) {
        println!("[MUXIVA][WARN][simulation.synthetic] real_audio=false real_ai=false network=disabled turns={turns} interval_ms={interval_ms} purpose=runtime-contract-test");
    }
    let notification_bus = if matches!(scenario, SimulationScenario::Voice) {
        let bus = NotificationBus::default();
        bus.subscribe(
            NamespacedName::new("muxiva.demo.speech.detected")
                .map_err(|error| error.to_string())?,
            |event| {
                println!(
                    "[MUXIVA][NOTIFICATION-BUS][subscriber] topic={} turn={} handler=session-observer",
                    event.data().topic(),
                    event.header().sequence_id().get()
                );
                Ok(())
            },
        )
        .map_err(|error| error.to_string())?;
        Some(bus)
    } else {
        None
    };
    run_graph(
        graph,
        &registry,
        Duration::from_millis(
            DEFAULT_RUN_TIMEOUT_MS.max(interval_ms.saturating_mul(u64::from(turns)) + 10_000),
        ),
        Duration::from_millis(DEFAULT_SHUTDOWN_TIMEOUT_MS),
        DEFAULT_RUN_TIMEOUT_MS.max(interval_ms.saturating_mul(u64::from(turns)) + 10_000),
        notification_bus,
    )
}

fn run_graph(
    graph: muxiva_core::GraphDefinition,
    registry: &NodeRegistry,
    timeout: Duration,
    shutdown_timeout: Duration,
    timeout_ms: u64,
    notification_bus: Option<NotificationBus>,
) -> Result<(), String> {
    let graph_id = graph.graph_id().as_str().to_owned();
    let node_total = graph.nodes().len();
    let edge_total = graph.edges().len();
    println!("[MUXIVA][INFO][graph.loaded] id={graph_id} nodes={node_total} edges={edge_total}");
    println!("[MUXIVA][GRAPH] human-readable DSL");
    print!("{}", graph.render_human_dsl());
    println!("[MUXIVA][INFO][runtime.started] mode=concurrent");
    let runtime = if let Some(notification_bus) = notification_bus {
        let nodes = materialize_registered_nodes(&graph, registry)
            .map_err(|error| format!("cannot materialize graph `{graph_id}`: {error}"))?;
        ConcurrentRuntime::new(graph, nodes, EdgePolicies::new(), RuntimeOptions::default())
            .map_err(|error| format!("cannot attach graph `{graph_id}`: {error}"))?
            .with_notification_bus(notification_bus)
            .start()
            .map_err(|error| format!("cannot start graph `{graph_id}`: {error}"))?
    } else {
        start_registered_runtime(
            graph,
            registry,
            EdgePolicies::new(),
            RuntimeOptions::default(),
        )
        .map_err(|error| format!("cannot start graph `{graph_id}`: {error}"))?
    };

    match runtime.wait(timeout) {
        Ok(summary) => {
            println!(
                "[MUXIVA][INFO][runtime.completed] status=success workers={}",
                summary.worker_total()
            );
            Ok(())
        }
        Err(RuntimeWaitError::Aborted(reason)) => Err(format!(
            "graph `{graph_id}` aborted: code={} category={:?} stage={:?} node={} message={}",
            reason.root().code(),
            reason.category(),
            reason.stage(),
            reason
                .node_id()
                .map(|node_id| node_id.as_str())
                .unwrap_or("<runtime>"),
            reason.root().message()
        )),
        Err(RuntimeWaitError::Timeout(diagnostics)) => {
            let active = diagnostics
                .active_nodes()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            runtime.stop();
            let cleanup = match runtime.wait(shutdown_timeout) {
                Err(RuntimeWaitError::Timeout(remaining)) => format!(
                    "cleanup timed out with active nodes [{}]",
                    remaining
                        .active_nodes()
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                _ => "cleanup completed".to_owned(),
            };
            Err(format!(
                "graph `{graph_id}` timed out after {timeout_ms} ms in state {:?} with active nodes [{active}]; {cleanup}",
                diagnostics.state()
            ))
        }
    }
}

fn cli_timeout(name: &str, milliseconds: u64) -> Result<Duration, String> {
    if milliseconds == 0 || milliseconds > MAX_CLI_TIMEOUT_MS {
        return Err(format!(
            "--{name} must be between 1 and {MAX_CLI_TIMEOUT_MS} milliseconds"
        ));
    }
    Ok(Duration::from_millis(milliseconds))
}

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = ProcessCommand::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = ProcessCommand::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = ProcessCommand::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    return Err("automatic browser opening is unsupported on this platform".into());

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn welcome() {
    println!("[MUXIVA] Real-time multimodal Agent Runtime");
    println!();
    println!("Start here:");
    println!("  muxiva init my-agent      Create a complete Muxiva project");
    println!("  muxiva serve graph.json   Run a long-lived Graph headlessly");
    println!("  muxiva run graph.json     Execute a finite Graph to completion");
    println!("  muxiva studio graph.json  Open the optional visual editor");
    println!("  muxiva doctor --voice     Check flagship voice-demo prerequisites");
    println!();
    println!("Real voice guide: {FLAGSHIP_GUIDE}");
    println!("Run `muxiva --help` for every command.");
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> (u8, u8, u8) {
    let chroma = value * saturation;
    let hue_sector = hue * 6.0;
    let secondary = chroma * (1.0 - ((hue_sector % 2.0) - 1.0).abs());
    let (r, g, b) = match hue_sector.floor() as i32 {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    let offset = value - chroma;
    (
        ((r + offset) * 255.0).round() as u8,
        ((g + offset) * 255.0).round() as u8,
        ((b + offset) * 255.0).round() as u8,
    )
}

fn logo_color(column: usize, width: usize) -> String {
    let hue = if width <= 1 {
        0.0
    } else {
        (column as f64 / (width - 1) as f64) * 0.78
    };
    let (r, g, b) = hsv_to_rgb(hue, 0.85, 0.95);
    format!("\u{1b}[38;2;{r};{g};{b}m")
}

fn print_logo() {
    let letters: [[&str; 7]; 6] = [
        [
            "█     █",
            "██   ██",
            "█ █ █ █",
            "█  █  █",
            "█     █",
            "█     █",
            "█     █",
        ],
        [
            "█     █",
            "█     █",
            "█     █",
            "█     █",
            "█     █",
            "█     █",
            " █████ ",
        ],
        [
            "█     █",
            " █   █ ",
            "  █ █  ",
            "   █   ",
            "  █ █  ",
            " █   █ ",
            "█     █",
        ],
        [
            "███████",
            "   █   ",
            "   █   ",
            "   █   ",
            "   █   ",
            "   █   ",
            "███████",
        ],
        [
            "█     █",
            "█     █",
            "█     █",
            "█     █",
            " █   █ ",
            "  █ █  ",
            "   █   ",
        ],
        [
            "   █   ",
            "  █ █  ",
            " █   █ ",
            "█     █",
            "███████",
            "█     █",
            "█     █",
        ],
    ];

    let width = letters.len() * 7 + (letters.len() - 1) * 2;
    let use_color = std::io::stdout().is_terminal();

    println!();
    for row in 0..7 {
        let mut line = String::new();
        for (index, letter) in letters.iter().enumerate() {
            if index > 0 {
                line.push_str("  ");
            }
            line.push_str(letter[row]);
        }
        if use_color {
            let mut colored = String::new();
            for (column, ch) in line.chars().enumerate() {
                if ch == '█' {
                    colored.push_str(&logo_color(column, width));
                    colored.push(ch);
                    colored.push_str("\u{1b}[0m");
                } else {
                    colored.push(ch);
                }
            }
            println!("{colored}");
        } else {
            println!("{line}");
        }
    }
    if use_color {
        println!("\u{1b}[2mReal-time multimodal Agent Runtime\u{1b}[0m");
    } else {
        println!("Real-time multimodal Agent Runtime");
    }
    println!();
}

fn main() {
    let result = match Cli::parse().command {
        None => {
            welcome();
            Ok(())
        }
        Some(Command::Studio {
            graph,
            port,
            host,
            no_open,
        }) => studio(graph, port, host, no_open),
        Some(Command::Init { path }) => init(&path),
        Some(Command::Validate { graph }) => validate(&graph)
            .map(|path| println!("[MUXIVA][INFO][graph.valid] path={}", path.display())),
        Some(Command::Run {
            graph,
            timeout_ms,
            shutdown_timeout_ms,
        }) => run(&graph, timeout_ms, shutdown_timeout_ms),
        Some(Command::Serve {
            graph,
            host,
            port,
            allowed_origins,
            shutdown_timeout_ms,
        }) => serve_graph(&graph, host, port, allowed_origins, shutdown_timeout_ms),
        Some(Command::Doctor { voice, strict }) => doctor::run(voice, strict),
        Some(Command::Simulate {
            scenario,
            turns,
            interval_ms,
        }) => simulate(scenario, turns, interval_ms),
        Some(Command::Demo {
            scenario,
            turns,
            interval_ms,
        }) => {
            eprintln!(
                "[MUXIVA][WARN][command.deprecated] `muxiva demo` is an offline synthetic fixture; use `muxiva simulate`"
            );
            simulate(scenario, turns, interval_ms)
        }
    };
    if let Err(error) = result {
        eprintln!("[MUXIVA][ERROR][command.failed] {error}");
        std::process::exit(2);
    }
}
