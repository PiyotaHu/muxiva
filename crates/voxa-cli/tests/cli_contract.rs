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
        .contains("[VOXA][INFO][graph.valid]"));

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
fn run_executes_the_initialized_graph_through_the_concurrent_runtime() {
    let directory = TestDirectory::new("run");
    let graph = directory.0.join("starter.json");
    assert!(voxa(&["init", graph.to_str().unwrap()], &directory.0)
        .status
        .success());

    let output = voxa(&["run", graph.to_str().unwrap()], &directory.0);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "[VOXA][INFO][graph.loaded] id=text-uppercase nodes=3 edges=2\n",
            "[VOXA][GRAPH] human-readable DSL\n",
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
            "[VOXA][INFO][runtime.started] mode=concurrent\n",
            "[VOXA][RESULT][sink] HELLO\n",
            "[VOXA][INFO][runtime.completed] status=success workers=3\n",
        )
    );
}

#[test]
fn installed_binary_has_a_self_contained_branded_demo_and_version() {
    let directory = TestDirectory::new("demo");
    let version = voxa(&["--version"], &directory.0);
    assert!(version.status.success());
    assert_eq!(String::from_utf8(version.stdout).unwrap(), "voxa 0.1.0\n");

    let output = voxa(&["demo"], &directory.0);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("[VOXA][INFO][demo.started] name=realtime-voice-agent\n"));
    assert!(stdout.contains("providers=mock network=disabled purpose=architecture-preview"));
    assert!(stdout.contains("graph \"realtime-voice-agent\""));
    assert!(stdout.contains("├─microphone.audio_out [audio] -> streaming-asr.audio_in"));
    assert!(stdout.contains("└─microphone.audio_out [audio] -> voice-activity.audio_in"));
    assert!(
        stdout.contains("[VOXA][JOIN][context-fusion] inputs=transcript+speech_event status=ready")
    );
    assert!(stdout.contains("[VOXA][RESULT][live-transcript] Voxa runs audio, ASR, VAD, reasoning, and TTS as one typed, concurrent graph."));
    assert!(stdout.contains("[VOXA][RESULT][speaker] played_audio_ms=20 provider=mock"));
    assert!(stdout.ends_with("[VOXA][INFO][runtime.completed] status=success workers=8\n"));

    let text = voxa(&["demo", "--scenario", "text"], &directory.0);
    assert!(text.status.success());
    let stdout = String::from_utf8(text.stdout).unwrap();
    assert!(stdout.contains("[VOXA][INFO][demo.started] name=text-uppercase"));
    assert!(stdout.contains("[VOXA][RESULT][sink] HELLO"));
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
        let output = voxa(&["run", "missing.json", flag, value], &directory.0);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("must be between 1 and 3600000 milliseconds"));
    }
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
