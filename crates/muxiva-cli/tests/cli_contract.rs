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
            "muxiva-cli-{scenario}-{}-{}",
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

fn muxiva(arguments: &[&str], current_dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_muxiva"))
        .args(arguments)
        .current_dir(current_dir)
        .output()
        .unwrap()
}

#[test]
fn help_and_bare_command_lead_with_actionable_product_entry_points() {
    let directory = TestDirectory::new("help");
    let help = muxiva(&["--help"], &directory.0);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for contract in [
        "studio    Open local visual Studio; auto-discover or create a workspace",
        "init      Create a Muxiva project with graph.json and project Node directories",
        "validate  Validate a project or Graph without executing any Node",
        "run       Execute a project or Graph with the concurrent Runtime",
        "serve     Run a long-lived Graph headlessly with a minimal browser bootstrap API",
        "doctor    Check local tools, project discovery, and optional voice-demo readiness",
        "simulate  Run synthetic, network-free Runtime fixtures for engineering checks",
    ] {
        assert!(help.contains(contract), "missing help contract: {contract}");
    }
    assert!(!help.contains("\n  demo"));
    assert!(help.contains("https://piyotahu.github.io/muxiva/voice-demo/"));

    let welcome = muxiva(&[], &directory.0);
    assert!(welcome.status.success());
    let welcome = String::from_utf8(welcome.stdout).unwrap();
    assert!(welcome.starts_with("[MUXIVA] Real-time multimodal Agent Runtime\n"));
    assert!(welcome.contains("muxiva serve graph.json"));
    assert!(welcome.contains("muxiva studio graph.json"));
    assert!(welcome.contains("muxiva init my-agent"));
    assert!(welcome.contains("muxiva doctor --voice"));
}

#[test]
fn init_creates_a_project_that_all_graph_commands_accept() {
    let directory = TestDirectory::new("project-init");
    let project = directory.0.join("my-agent");

    let created = muxiva(&["init", project.to_str().unwrap()], &directory.0);
    assert!(created.status.success());
    assert!(project.join("graph.json").is_file());
    assert!(project.join("README.md").is_file());
    assert!(fs::read_to_string(project.join(".gitignore"))
        .unwrap()
        .contains(".muxiva/observability/"));
    assert!(project.join(".muxiva/nodes").is_dir());
    assert!(project.join(".muxiva/templates").is_dir());

    assert!(
        muxiva(&["validate", project.to_str().unwrap()], &directory.0)
            .status
            .success()
    );
    assert!(muxiva(&["run", project.to_str().unwrap()], &directory.0)
        .status
        .success());
    let refused = muxiva(&["init", project.to_str().unwrap()], &directory.0);
    assert_eq!(refused.status.code(), Some(2));
    assert!(String::from_utf8(refused.stderr)
        .unwrap()
        .contains("refusing to overwrite"));
}

#[test]
fn init_is_create_only_and_its_output_validates_with_the_same_cli() {
    let directory = TestDirectory::new("init");
    let graph = directory.0.join("starter.json");

    let created = muxiva(&["init", graph.to_str().unwrap()], &directory.0);
    assert!(created.status.success());
    let validate = muxiva(&["validate", graph.to_str().unwrap()], &directory.0);
    assert!(validate.status.success());
    assert!(String::from_utf8(validate.stdout)
        .unwrap()
        .contains("[MUXIVA][INFO][graph.valid]"));

    let original = fs::read(&graph).unwrap();
    let refused = muxiva(&["init", graph.to_str().unwrap()], &directory.0);
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
        r#"{"version":"muxiva.graph/v1","graph_id":"bad","nodes":[],"edges":[],"unknown":true}"#,
    )
    .unwrap();

    let validate = muxiva(&["validate", graph.to_str().unwrap()], &directory.0);
    let run = muxiva(&["run", graph.to_str().unwrap()], &directory.0);
    assert_eq!(validate.status.code(), Some(2));
    assert_eq!(run.status.code(), Some(2));
    assert_eq!(validate.stderr, run.stderr);
    assert!(String::from_utf8(validate.stderr)
        .unwrap()
        .contains("MUXIVA-GRAPH-JSON"));
}

#[test]
fn run_executes_the_initialized_graph_through_the_concurrent_runtime() {
    let directory = TestDirectory::new("run");
    let graph = directory.0.join("starter.json");
    assert!(muxiva(&["init", graph.to_str().unwrap()], &directory.0)
        .status
        .success());

    let output = muxiva(&["run", graph.to_str().unwrap()], &directory.0);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "[MUXIVA][INFO][graph.loaded] id=text-uppercase nodes=3 edges=2\n",
            "[MUXIVA][GRAPH] human-readable DSL\n",
            "graph \"text-uppercase\" {\n",
            "  node \"sink\" kind=sink type=\"builtin.stdout_text_sink\"\n",
            "    input text_in: text\n",
            "  node \"source\" kind=source type=\"builtin.text_source\"\n",
            "    output text_out: text\n",
            "  node \"upper\" kind=transform type=\"builtin.uppercase\"\n",
            "    input text_in: text\n",
            "    output text_out: text\n",
            "  edge \"source-upper\" source.text_out -> upper.text_in frame=text queue=32/block\n",
            "  edge \"upper-sink\" upper.text_out -> sink.text_in frame=text queue=32/block\n",
            "}\n",
            "flow:\n",
            "  source\n",
            "    └─source.text_out [text] -> upper.text_in\n",
            "  upper\n",
            "    └─upper.text_out [text] -> sink.text_in\n",
            "[MUXIVA][INFO][runtime.started] mode=concurrent\n",
            "[MUXIVA][RESULT][sink] HELLO\n",
            "[MUXIVA][INFO][runtime.completed] status=success workers=3\n",
        )
    );
}

#[test]
fn run_rejects_missing_project_credentials_before_starting_node_hosts() {
    let directory = TestDirectory::new("run-credential-preflight");
    let project = directory.0.join("agent");
    assert!(muxiva(&["init", project.to_str().unwrap()], &directory.0)
        .status
        .success());
    let package = project.join(".muxiva/nodes/requires_key");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("muxiva.node.json"),
        r#"{"format":"muxiva.node/v1","package_id":"requires_key","display_name":"Requires Key","node_type":"test.requires_key","language":"python","factory_version":"1.0.0","kind":"source","entrypoint":"node:Node","ports":[],"config_schema":{"type":"object"},"connection":{"id":"test_service","display_name":"Test Service","description":"Test-only connection","fields":[{"name":"api_key","label":"API Key","environment":"MUXIVA_TEST_REQUIRED_CLI_KEY","secret":true,"required":true,"default":""}]}}"#,
    )
    .unwrap();
    fs::write(package.join("node.py"), "class Node:\n    pass\n").unwrap();
    fs::write(
        project.join("graph.json"),
        r#"{"version":"muxiva.graph/v1","graph_id":"preflight","nodes":[{"id":"guarded","node_type":"test.requires_key","language":"python","factory_version":"1.0.0","node_config":{}}],"edges":[]}"#,
    )
    .unwrap();

    let output = muxiva(&["run", project.to_str().unwrap()], &directory.0);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("required project credentials are missing"));
    assert!(error.contains("Test Service / API Key (MUXIVA_TEST_REQUIRED_CLI_KEY)"));
    assert!(error.contains(project.join(".env").to_str().unwrap()));
    assert!(error.contains("Studio is optional"));
    assert!(!error.contains("Project Node Host"));
}

#[test]
fn installed_binary_labels_synthetic_simulation_and_reports_version() {
    let directory = TestDirectory::new("simulate");
    let version = muxiva(&["--version"], &directory.0);
    assert!(version.status.success());
    assert_eq!(String::from_utf8(version.stdout).unwrap(), "muxiva 0.1.0\n");

    let output = muxiva(&["simulate"], &directory.0);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("[MUXIVA][INFO][simulation.started] fixture=realtime-voice-agent\n"));
    assert!(stdout.contains(
        "real_audio=false real_ai=false network=disabled turns=4 interval_ms=650 purpose=runtime-contract-test"
    ));
    assert!(stdout.contains("graph \"realtime-voice-agent\""));
    assert!(stdout.contains("├─microphone.audio_out [audio] -> streaming-asr.audio_in"));
    assert!(stdout.contains("└─microphone.audio_out [audio] -> voice-activity.audio_in"));
    assert!(stdout.contains("[MUXIVA][TURN][started] turn=1 of=4"));
    assert!(stdout.contains("[MUXIVA][TURN][started] turn=4 of=4"));
    assert!(
        stdout.contains("[MUXIVA][NOTIFICATION-BUS][publish] topic=muxiva.demo.speech.detected")
    );
    assert!(stdout.contains("[MUXIVA][SIGNAL][received] node=context-fusion"));
    assert!(stdout.contains(
        "[MUXIVA][JOIN][context-fusion] turn=4 inputs=transcript+speech_event status=ready"
    ));
    assert!(stdout
        .contains("[MUXIVA][RESULT][live-transcript] This session completed four voice turns"));
    assert!(stdout.contains("[MUXIVA][RESULT][speaker] played_audio_ms=20 provider=mock"));
    assert!(stdout.ends_with("[MUXIVA][INFO][runtime.completed] status=success workers=8\n"));

    let text = muxiva(&["simulate", "--scenario", "text"], &directory.0);
    assert!(text.status.success());
    let stdout = String::from_utf8(text.stdout).unwrap();
    assert!(stdout.contains("[MUXIVA][INFO][simulation.started] fixture=text-uppercase"));
    assert!(stdout.contains("[MUXIVA][RESULT][sink] HELLO"));

    let legacy = muxiva(&["demo", "--scenario", "text"], &directory.0);
    assert!(legacy.status.success());
    assert!(String::from_utf8(legacy.stderr)
        .unwrap()
        .contains("`muxiva demo` is an offline synthetic fixture"));
}

#[test]
fn doctor_is_redacted_and_actionable_without_external_credentials() {
    let directory = TestDirectory::new("doctor");
    let output = muxiva(&["doctor"], &directory.0);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[MUXIVA][DOCTOR][PASS] cli version=0.1.0"));
    assert!(stdout.contains("next=\"muxiva init my-agent\""));
    assert!(stdout.contains("[MUXIVA][DOCTOR][SUMMARY]"));
}

#[test]
fn run_rejects_unbounded_or_zero_wait_configuration_before_loading() {
    let directory = TestDirectory::new("run-timeouts");

    for (flag, value) in [
        ("--timeout-ms", "0"),
        ("--timeout-ms", "3600001"),
        ("--shutdown-timeout-ms", "0"),
        ("--shutdown-timeout-ms", "3600001"),
    ] {
        let output = muxiva(&["run", "missing.json", flag, value], &directory.0);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("must be between 1 and 3600000 milliseconds"));
    }
}

#[test]
fn headless_serve_requires_explicit_remote_security_boundaries() {
    let directory = TestDirectory::new("serve-security");
    let project = directory.0.join("agent");
    assert!(muxiva(&["init", project.to_str().unwrap()], &directory.0)
        .status
        .success());

    let public = muxiva(
        &[
            "serve",
            project.to_str().unwrap(),
            "--host",
            "0.0.0.0",
            "--port",
            "0",
        ],
        &directory.0,
    );
    assert!(!public.status.success());
    assert!(String::from_utf8(public.stderr)
        .unwrap()
        .contains("non-loopback client API requires MUXIVA_CLIENT_API_TOKEN"));

    let wildcard = muxiva(
        &["serve", project.to_str().unwrap(), "--allow-origin", "*"],
        &directory.0,
    );
    assert!(!wildcard.status.success());
    assert!(String::from_utf8(wildcard.stderr)
        .unwrap()
        .contains("--allow-origin '*' is not accepted"));
}

#[test]
fn studio_reports_an_exact_requested_port_collision() {
    let directory = TestDirectory::new("port");
    let graph = directory.0.join("starter.json");
    assert!(muxiva(&["init", graph.to_str().unwrap()], &directory.0)
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
    let output = muxiva(
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

#[test]
fn studio_without_arguments_creates_and_selects_a_local_workspace() {
    let directory = TestDirectory::new("studio-default");
    let reservation = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("SKIP loopback port contract: sandbox denies socket binding");
            return;
        }
        Err(error) => panic!("failed to reserve loopback port: {error}"),
    };
    let port = reservation.local_addr().unwrap().port().to_string();

    let output = muxiva(&["studio", "--port", &port, "--no-open"], &directory.0);

    assert_eq!(output.status.code(), Some(2));
    assert!(directory.0.join("muxiva.graph.json").is_file());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("[MUXIVA][INFO][studio.workspace] mode=created"));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains(&format!("cannot bind 127.0.0.1:{port}")));
}
