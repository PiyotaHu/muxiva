use clap::{Parser, Subcommand};
use std::{
    fs,
    net::{IpAddr, TcpListener},
    path::PathBuf,
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
    Studio {
        graph: PathBuf,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long)]
        no_open: bool,
    },
}
fn load(path: &PathBuf) -> Result<(), String> {
    let data = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = voxa_graph_json::parse(&data).map_err(render)?;
    voxa_graph_json::compile(&doc).map(|_| ()).map_err(render)
}
fn render(errors: Vec<voxa_graph_json::GraphDiagnostic>) -> String {
    errors
        .into_iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
fn main() {
    let cli = Cli::parse();
    let result=match cli.command {Command::Init{path}=>{if path.exists(){Err(format!("refusing to overwrite {}",path.display()))}else{fs::write(&path,STARTER).map_err(|e|e.to_string()).map(|_|println!("created {}; configure credentials outside Graph v1",path.display()))}},Command::Validate{graph}=>load(&graph).map(|_|println!("valid: {}",graph.display())),Command::Run{graph}=>load(&graph).map(|_|println!("validated graph {}; runnable factories are intentionally limited to compiled-in builtins",graph.display())),Command::Studio{graph,port,host,no_open:_}=>load(&graph).and_then(|_|{if !host.is_loopback(){eprintln!("WARNING: Studio is binding a non-loopback address; access token protections are required.")}let requested=port.unwrap_or(0);let listener=TcpListener::bind((host,requested)).map_err(|e|format!("cannot bind {host}:{requested}: {e}"))?;let addr=listener.local_addr().map_err(|e|e.to_string())?;let token=voxa_studio::random_token().map_err(|e|e.to_string())?;println!("Studio listening at http://{addr}/#{token}");voxa_studio::serve(listener,graph,token).map_err(|e|e.to_string())})};
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(2)
    }
}
