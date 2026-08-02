use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use super::{discover_studio_graph, FLAGSHIP_GUIDE};

fn tool_version(command: &str) -> Option<String> {
    let output = Command::new(command).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout
        .lines()
        .chain(stderr.lines())
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_owned())
}

fn check_tool(label: &str, commands: &[&str], warnings: &mut usize) {
    for command in commands {
        if let Some(version) = tool_version(command) {
            println!("[VOXA][DOCTOR][PASS] {label} command={command} version={version}");
            return;
        }
    }
    *warnings += 1;
    println!(
        "[VOXA][DOCTOR][WARN] {label} missing commands={}",
        commands.join(",")
    );
}

fn voice_project(current: &Path) -> Option<PathBuf> {
    for ancestor in current.ancestors() {
        if ancestor.join(".voxa/providers.json").is_file()
            && ancestor
                .join(".voxa/templates/01-qwen-realtime.json")
                .is_file()
        {
            return Some(ancestor.to_owned());
        }
        let candidate = ancestor.join("examples/voice-agent");
        if candidate.join(".voxa/providers.json").is_file()
            && candidate
                .join(".voxa/templates/01-qwen-realtime.json")
                .is_file()
        {
            return Some(candidate);
        }
    }
    None
}

fn native_node_pack_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libvoxa_node_pack.dylib"
    } else if cfg!(target_os = "windows") {
        "voxa_node_pack.dll"
    } else {
        "libvoxa_node_pack.so"
    }
}

fn inspect_voice_project(current: &Path, warnings: &mut usize) -> bool {
    let Some(project) = voice_project(current) else {
        *warnings += 1;
        println!(
            "[VOXA][DOCTOR][WARN] voice-project missing next=\"clone Voxa and open examples/voice-agent\""
        );
        return false;
    };
    println!(
        "[VOXA][DOCTOR][PASS] voice-project root={}",
        project.display()
    );
    for package in ["agora_audio_source", "agora_audio_sink"] {
        let package_root = project.join(".voxa/native").join(package);
        let artifact = package_root.join(native_node_pack_filename());
        let mode = fs::read_to_string(package_root.join("provider-mode"))
            .unwrap_or_else(|_| "unknown".into());
        if artifact.is_file() && mode.trim() == "agora-native" {
            println!(
                "[VOXA][DOCTOR][PASS] native-node-pack package={package} mode=agora-native artifact={}",
                artifact.display()
            );
        } else {
            *warnings += 1;
            println!(
                "[VOXA][DOCTOR][WARN] native-node-pack package={package} mode={} ready=false next=\"./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk\"",
                mode.trim()
            );
        }
    }
    true
}

fn report_credentials() {
    let names = [
        "DASHSCOPE_API_KEY",
        "DASHSCOPE_WORKSPACE_ID",
        "VOXA_AGORA_APP_ID",
        "VOXA_AGORA_CHANNEL",
        "VOXA_AGORA_SOURCE_TOKEN",
        "VOXA_AGORA_SINK_TOKEN",
        "VOXA_AGORA_WEB_TOKEN",
    ];
    let configured = names
        .iter()
        .filter(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
        .count();
    println!(
        "[VOXA][DOCTOR][INFO] voice-credentials environment={configured}/{} configure=Studio-Connections-or-environment values=redacted",
        names.len()
    );
}

pub(super) fn run(voice: bool, strict: bool) -> Result<(), String> {
    let current = env::current_dir().map_err(|error| error.to_string())?;
    let mut warnings = 0usize;
    println!(
        "[VOXA][DOCTOR][PASS] cli version={} platform={}-{}",
        env!("CARGO_PKG_VERSION"),
        env::consts::OS,
        env::consts::ARCH
    );
    if let Some(graph) = discover_studio_graph(&current) {
        println!("[VOXA][DOCTOR][PASS] workspace graph={}", graph.display());
    } else {
        println!("[VOXA][DOCTOR][INFO] workspace graph=not-found next=\"voxa init my-agent\"");
    }
    check_tool("Python", &["python3", "python"], &mut warnings);
    check_tool("CMake", &["cmake"], &mut warnings);
    check_tool("C++ compiler", &["c++", "clang++", "g++"], &mut warnings);

    if voice && inspect_voice_project(&current, &mut warnings) {
        report_credentials();
    }

    println!("[VOXA][DOCTOR][SUMMARY] warnings={warnings} guide={FLAGSHIP_GUIDE}");
    if strict && warnings > 0 {
        Err(format!("doctor found {warnings} warning(s)"))
    } else {
        Ok(())
    }
}
