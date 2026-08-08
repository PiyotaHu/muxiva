use clap::{Parser, Subcommand, ValueEnum};
use muxiva_core::{
    materialize_registered_nodes, start_registered_runtime, ConcurrentRuntime, EdgePolicies,
    EventBus, NodeRegistry, RuntimeOptions, RuntimeWaitError,
};
use muxiva_types::NamespacedName;
use std::{
    env, fs,
    net::{IpAddr, TcpListener},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};

mod doctor;

const DEFAULT_RUN_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 5_000;
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
muxiva studio .
muxiva run .
```

Build a real voice assistant with the flagship guide:
https://piyotahu.github.io/muxiva/voice-demo/
"#;

#[derive(Parser)]
#[command(
    name = "muxiva",
    version,
    about = "Build and run real-time multimodal agent graphs",
    long_about = "Muxiva is a real-time multimodal Agent Runtime for typed, concurrent graphs.\n\nStart Studio without arguments, create a project, or inspect your environment with doctor.",
    after_help = "Start here:\n  muxiva studio          Open or create a local Studio workspace\n  muxiva init my-agent   Create a complete Muxiva project\n  muxiva doctor --voice  Check flagship voice-demo prerequisites\n\nReal voice guide: https://piyotahu.github.io/muxiva/voice-demo/"
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
    }
    println!(
        "[MUXIVA][INFO][project.created] root={} graph={} credentials=external",
        path.display(),
        graph.display()
    );
    println!("[MUXIVA][NEXT] muxiva studio {}", path.display());
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
    let token = muxiva_studio::random_token().map_err(|error| error.to_string())?;
    let url = format!("http://{address}/#{token}");
    println!("[MUXIVA][INFO][studio.ready] url={url}");
    println!("[MUXIVA][INFO][studio.graph] path={}", graph.display());
    if !no_open {
        if let Err(error) = open_browser(&url) {
            eprintln!("[MUXIVA][WARN][studio.browser-open] {error}");
        }
    }
    muxiva_studio::serve(listener, graph, token).map_err(|error| error.to_string())
}

fn run(graph_path: &Path, timeout_ms: u64, shutdown_timeout_ms: u64) -> Result<(), String> {
    let timeout = cli_timeout("timeout-ms", timeout_ms)?;
    let shutdown_timeout = cli_timeout("shutdown-timeout-ms", shutdown_timeout_ms)?;
    let graph_path = resolve_graph_path(graph_path)?;
    let registry = muxiva_studio::project_registry(&graph_path)?;
    let graph = load(&graph_path, &registry)?;
    run_graph(
        graph,
        &registry,
        timeout,
        shutdown_timeout,
        timeout_ms,
        None,
    )
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
    let event_bus = if matches!(scenario, SimulationScenario::Voice) {
        let bus = EventBus::default();
        bus.subscribe(
            NamespacedName::new("muxiva.demo.speech.detected")
                .map_err(|error| error.to_string())?,
            |event| {
                println!(
                    "[MUXIVA][EVENTBUS][subscriber] topic={} turn={} handler=session-observer",
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
        event_bus,
    )
}

fn run_graph(
    graph: muxiva_core::GraphDefinition,
    registry: &NodeRegistry,
    timeout: Duration,
    shutdown_timeout: Duration,
    timeout_ms: u64,
    event_bus: Option<EventBus>,
) -> Result<(), String> {
    let graph_id = graph.graph_id().as_str().to_owned();
    let node_total = graph.nodes().len();
    let edge_total = graph.edges().len();
    println!("[MUXIVA][INFO][graph.loaded] id={graph_id} nodes={node_total} edges={edge_total}");
    println!("[MUXIVA][GRAPH] human-readable DSL");
    print!("{}", graph.render_human_dsl());
    println!("[MUXIVA][INFO][runtime.started] mode=concurrent");
    let runtime = if let Some(event_bus) = event_bus {
        let nodes = materialize_registered_nodes(&graph, registry)
            .map_err(|error| format!("cannot materialize graph `{graph_id}`: {error}"))?;
        ConcurrentRuntime::new(graph, nodes, EdgePolicies::new(), RuntimeOptions::default())
            .map_err(|error| format!("cannot attach graph `{graph_id}`: {error}"))?
            .with_event_bus(event_bus)
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
    println!("  muxiva studio          Open or create a local Studio workspace");
    println!("  muxiva init my-agent   Create a complete Muxiva project");
    println!("  muxiva doctor --voice  Check flagship voice-demo prerequisites");
    println!();
    println!("Real voice guide: {FLAGSHIP_GUIDE}");
    println!("Run `muxiva --help` for every command.");
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
