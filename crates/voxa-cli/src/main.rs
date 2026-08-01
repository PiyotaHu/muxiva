use clap::{Parser, Subcommand, ValueEnum};
use std::{
    fs,
    net::{IpAddr, TcpListener},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    time::Duration,
};
use voxa_core::{
    start_registered_runtime, EdgePolicies, NodeRegistry, RuntimeOptions, RuntimeWaitError,
};

const DEFAULT_RUN_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_SHUTDOWN_TIMEOUT_MS: u64 = 5_000;
const MAX_CLI_TIMEOUT_MS: u64 = 60 * 60 * 1_000;

const STARTER: &str = r#"{
  "version": "voxa.graph/v1",
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

#[derive(Parser)]
#[command(
    name = "voxa",
    version,
    about = "Build and run real-time multimodal agent graphs"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a self-contained product demo without requiring a project checkout.
    Demo {
        /// Demo journey: voice shows fork/join multimodal execution; text is a basic smoke test.
        #[arg(long, value_enum, default_value_t = DemoScenario::Voice)]
        scenario: DemoScenario,
    },
    Init {
        #[arg(default_value = "voxa.graph.json")]
        path: PathBuf,
    },
    Validate {
        graph: PathBuf,
    },
    Run {
        graph: PathBuf,
        /// Maximum wall-clock time to wait for graph completion.
        #[arg(long, default_value_t = DEFAULT_RUN_TIMEOUT_MS)]
        timeout_ms: u64,
        /// Bounded cleanup wait after a run timeout.
        #[arg(long, default_value_t = DEFAULT_SHUTDOWN_TIMEOUT_MS)]
        shutdown_timeout_ms: u64,
    },
    /// Launch the local visual Graph v1 editor.
    Studio {
        graph: PathBuf,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        /// Print the URL without opening the default browser.
        #[arg(long)]
        no_open: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum DemoScenario {
    Voice,
    Text,
}

fn load(path: &Path, registry: &NodeRegistry) -> Result<voxa_core::GraphDefinition, String> {
    let data = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    load_source(&data, registry)
}

fn load_source(data: &str, registry: &NodeRegistry) -> Result<voxa_core::GraphDefinition, String> {
    let document = voxa_graph_json::parse(data).map_err(render)?;
    voxa_graph_json::compile_with_registry(&document, registry).map_err(render)
}

fn validate(path: &Path) -> Result<(), String> {
    load(path, &voxa_graph_json::builtin_registry()).map(|_| ())
}

fn render(errors: Vec<voxa_graph_json::GraphDiagnostic>) -> String {
    errors
        .into_iter()
        .map(|diagnostic| diagnostic.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn init(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("refusing to overwrite {}", path.display()));
    }
    fs::write(path, STARTER).map_err(|error| error.to_string())?;
    println!(
        "[VOXA][INFO][graph.created] path={} credentials=external",
        path.display()
    );
    Ok(())
}

fn studio(graph: PathBuf, port: Option<u16>, host: IpAddr, no_open: bool) -> Result<(), String> {
    validate(&graph)?;
    let graph = fs::canonicalize(&graph)
        .map_err(|error| format!("cannot resolve {}: {error}", graph.display()))?;
    if !host.is_loopback() {
        eprintln!("[VOXA][WARN][studio.non-loopback] access token protections are required");
    }
    let requested = port.unwrap_or(0);
    let listener = TcpListener::bind((host, requested))
        .map_err(|error| format!("cannot bind {host}:{requested}: {error}"))?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let token = voxa_studio::random_token().map_err(|error| error.to_string())?;
    let url = format!("http://{address}/#{token}");
    println!("[VOXA][INFO][studio.ready] url={url}");
    println!("[VOXA][INFO][studio.graph] path={}", graph.display());
    if !no_open {
        if let Err(error) = open_browser(&url) {
            eprintln!("[VOXA][WARN][studio.browser-open] {error}");
        }
    }
    voxa_studio::serve(listener, graph, token).map_err(|error| error.to_string())
}

fn run(graph_path: &Path, timeout_ms: u64, shutdown_timeout_ms: u64) -> Result<(), String> {
    let timeout = cli_timeout("timeout-ms", timeout_ms)?;
    let shutdown_timeout = cli_timeout("shutdown-timeout-ms", shutdown_timeout_ms)?;
    let registry = voxa_graph_json::builtin_registry();
    let graph = load(graph_path, &registry)?;
    run_graph(graph, &registry, timeout, shutdown_timeout, timeout_ms)
}

fn demo(scenario: DemoScenario) -> Result<(), String> {
    let registry = voxa_graph_json::builtin_registry();
    let (name, source) = match scenario {
        DemoScenario::Voice => ("realtime-voice-agent", VOICE_DEMO),
        DemoScenario::Text => ("text-uppercase", STARTER),
    };
    let graph = load_source(source, &registry)?;
    println!("[VOXA][INFO][demo.started] name={name}");
    if matches!(scenario, DemoScenario::Voice) {
        println!(
            "[VOXA][INFO][demo.mode] providers=mock network=disabled purpose=architecture-preview"
        );
    }
    run_graph(
        graph,
        &registry,
        Duration::from_millis(DEFAULT_RUN_TIMEOUT_MS),
        Duration::from_millis(DEFAULT_SHUTDOWN_TIMEOUT_MS),
        DEFAULT_RUN_TIMEOUT_MS,
    )
}

fn run_graph(
    graph: voxa_core::GraphDefinition,
    registry: &NodeRegistry,
    timeout: Duration,
    shutdown_timeout: Duration,
    timeout_ms: u64,
) -> Result<(), String> {
    let graph_id = graph.graph_id().as_str().to_owned();
    let node_total = graph.nodes().len();
    let edge_total = graph.edges().len();
    println!("[VOXA][INFO][graph.loaded] id={graph_id} nodes={node_total} edges={edge_total}");
    println!("[VOXA][GRAPH] human-readable DSL");
    print!("{}", graph.render_human_dsl());
    println!("[VOXA][INFO][runtime.started] mode=concurrent");
    let runtime = start_registered_runtime(
        graph,
        registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .map_err(|error| format!("cannot start graph `{graph_id}`: {error}"))?;

    match runtime.wait(timeout) {
        Ok(summary) => {
            println!(
                "[VOXA][INFO][runtime.completed] status=success workers={}",
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

fn main() {
    let result = match Cli::parse().command {
        Command::Demo { scenario } => demo(scenario),
        Command::Init { path } => init(&path),
        Command::Validate { graph } => {
            validate(&graph).map(|_| println!("[VOXA][INFO][graph.valid] path={}", graph.display()))
        }
        Command::Run {
            graph,
            timeout_ms,
            shutdown_timeout_ms,
        } => run(&graph, timeout_ms, shutdown_timeout_ms),
        Command::Studio {
            graph,
            port,
            host,
            no_open,
        } => studio(graph, port, host, no_open),
    };
    if let Err(error) = result {
        eprintln!("[VOXA][ERROR][command.failed] {error}");
        std::process::exit(2);
    }
}
