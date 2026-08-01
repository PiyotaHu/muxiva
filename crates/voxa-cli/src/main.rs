use clap::{Parser, Subcommand};
use std::{
    fs,
    net::{IpAddr, TcpListener},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

const STARTER: &str = r#"{
  "version": "voxa.graph/v1",
  "graph_id": "text-uppercase",
  "nodes": [
    {"id":"source","node_type":"builtin.text_source","language":"rust","node_config":{"text":"hello"}},
    {"id":"upper","node_type":"builtin.uppercase","language":"rust","node_config":{}},
    {"id":"sink","node_type":"builtin.text_sink","language":"rust","node_config":{}}
  ],
  "edges": [
    {"id":"source-upper","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"upper","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":32,"overflow":"block"}},
    {"id":"upper-sink","from":{"node_id":"upper","port":"text_out"},"to":{"node_id":"sink","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":32,"overflow":"block"}}
  ]
}"#;

#[derive(Parser)]
#[command(name = "voxa", about = "Voxa local graph tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(default_value = "voxa.graph.json")]
        path: PathBuf,
    },
    Validate {
        graph: PathBuf,
    },
    Run {
        graph: PathBuf,
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

fn load(path: &Path) -> Result<(), String> {
    let data = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let document = voxa_graph_json::parse(&data).map_err(render)?;
    voxa_graph_json::compile(&document)
        .map(|_| ())
        .map_err(render)
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
        "created {}; configure credentials outside Graph v1",
        path.display()
    );
    Ok(())
}

fn studio(graph: PathBuf, port: Option<u16>, host: IpAddr, no_open: bool) -> Result<(), String> {
    load(&graph)?;
    let graph = fs::canonicalize(&graph)
        .map_err(|error| format!("cannot resolve {}: {error}", graph.display()))?;
    if !host.is_loopback() {
        eprintln!(
            "WARNING: Studio is binding a non-loopback address; access token protections are required."
        );
    }
    let requested = port.unwrap_or(0);
    let listener = TcpListener::bind((host, requested))
        .map_err(|error| format!("cannot bind {host}:{requested}: {error}"))?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let token = voxa_studio::random_token().map_err(|error| error.to_string())?;
    let url = format!("http://{address}/#{token}");
    println!("Voxa Studio visual editor: {url}");
    println!("Editing: {}", graph.display());
    if !no_open {
        if let Err(error) = open_browser(&url) {
            eprintln!("WARNING: could not open the browser automatically: {error}");
        }
    }
    voxa_studio::serve(listener, graph, token).map_err(|error| error.to_string())
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
        Command::Init { path } => init(&path),
        Command::Validate { graph } => load(&graph).map(|_| println!("valid: {}", graph.display())),
        Command::Run { graph } => load(&graph).map(|_| {
            println!(
                "validated graph {}; runnable factories are intentionally limited to compiled-in builtins",
                graph.display()
            )
        }),
        Command::Studio {
            graph,
            port,
            host,
            no_open,
        } => studio(graph, port, host, no_open),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(2);
    }
}
