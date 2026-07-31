use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(scenario: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "voxa-cli-{scenario}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn voxa(arguments: &[&str], current_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_voxa"))
        .args(arguments)
        .current_dir(current_dir)
        .output()
        .unwrap()
}

#[test]
fn init_is_create_only_and_its_output_validates_with_the_same_cli() {
    let directory = TestDirectory::new("init");
    let graph = directory.0.join("starter.json");

    let created = voxa(&["init", graph.to_str().unwrap()], &directory.0);
    assert!(created.status.success());
    let validate = voxa(&["validate", graph.to_str().unwrap()], &directory.0);
    assert!(validate.status.success());
    assert!(String::from_utf8(validate.stdout)
        .unwrap()
        .contains("valid:"));

    let original = fs::read(&graph).unwrap();
    let refused = voxa(&["init", graph.to_str().unwrap()], &directory.0);
    assert_eq!(refused.status.code(), Some(2));
    assert!(String::from_utf8(refused.stderr)
        .unwrap()
        .contains("refusing to overwrite"));
    assert_eq!(fs::read(graph).unwrap(), original);
}

#[test]
fn validate_and_run_report_the_same_graph_diagnostic() {
    let directory = TestDirectory::new("diagnostic");
    let graph = directory.0.join("invalid.json");
    fs::write(
        &graph,
        r#"{"version":"voxa.graph/v1","graph_id":"bad","nodes":[],"edges":[],"unknown":true}"#,
    )
    .unwrap();

    let validate = voxa(&["validate", graph.to_str().unwrap()], &directory.0);
    let run = voxa(&["run", graph.to_str().unwrap()], &directory.0);
    assert_eq!(validate.status.code(), Some(2));
    assert_eq!(run.status.code(), Some(2));
    assert_eq!(validate.stderr, run.stderr);
    assert!(String::from_utf8(validate.stderr)
        .unwrap()
        .contains("VOXA-GRAPH-JSON"));
}

#[test]
fn studio_reports_an_exact_requested_port_collision() {
    let directory = TestDirectory::new("port");
    let graph = directory.0.join("starter.json");
    assert!(voxa(&["init", graph.to_str().unwrap()], &directory.0)
        .status
        .success());

    let reservation = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("SKIP loopback port contract: sandbox denies socket binding");
            return;
        }
        Err(error) => panic!("failed to reserve loopback port: {error}"),
    };
    let port = reservation.local_addr().unwrap().port().to_string();
    let output = voxa(
        &[
            "studio",
            graph.to_str().unwrap(),
            "--host",
            "127.0.0.1",
            "--port",
            &port,
            "--no-open",
        ],
        &directory.0,
    );

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&format!("cannot bind 127.0.0.1:{port}")));
}
