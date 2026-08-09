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

fn package_version(path: &Path) -> Option<String> {
    let document: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    document.get("version")?.as_str().map(ToOwned::to_owned)
}

fn check_tool(label: &str, commands: &[&str], warnings: &mut usize) {
    for command in commands {
        if let Some(version) = tool_version(command) {
            println!("[MUXIVA][DOCTOR][PASS] {label} command={command} version={version}");
            return;
        }
    }
    *warnings += 1;
    println!(
        "[MUXIVA][DOCTOR][WARN] {label} missing commands={}",
        commands.join(",")
    );
}

fn voice_project(current: &Path) -> Option<PathBuf> {
    for ancestor in current.ancestors() {
        if ancestor.join(".muxiva/providers.json").is_file()
            && ancestor
                .join(".muxiva/templates/01-qwen-realtime.json")
                .is_file()
        {
            return Some(ancestor.to_owned());
        }
        let candidate = ancestor.join("examples/voice-agent");
        if candidate.join(".muxiva/providers.json").is_file()
            && candidate
                .join(".muxiva/templates/01-qwen-realtime.json")
                .is_file()
        {
            return Some(candidate);
        }
    }
    None
}

fn native_node_pack_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libmuxiva_node_pack.dylib"
    } else if cfg!(target_os = "windows") {
        "muxiva_node_pack.dll"
    } else {
        "libmuxiva_node_pack.so"
    }
}

fn inspect_voice_project(current: &Path, warnings: &mut usize) -> Option<PathBuf> {
    let Some(project) = voice_project(current) else {
        *warnings += 1;
        println!(
            "[MUXIVA][DOCTOR][WARN] voice-project missing next=\"clone Muxiva and open examples/voice-agent\""
        );
        return None;
    };
    println!(
        "[MUXIVA][DOCTOR][PASS] voice-project root={}",
        project.display()
    );
    for package in [
        "agora_audio_source",
        "agora_audio_sink",
        "agora_data_source",
        "agora_data_sink",
    ] {
        let package_root = project.join(".muxiva/native").join(package);
        let artifact = package_root.join(native_node_pack_filename());
        let mode = fs::read_to_string(package_root.join("provider-mode"))
            .unwrap_or_else(|_| "unknown".into());
        if artifact.is_file() && mode.trim() == "agora-native" {
            println!(
                "[MUXIVA][DOCTOR][PASS] native-node-pack package={package} mode=agora-native artifact={}",
                artifact.display()
            );
        } else {
            *warnings += 1;
            println!(
                "[MUXIVA][DOCTOR][WARN] native-node-pack package={package} mode={} ready=false next=\"./examples/voice-agent/setup.sh\"",
                mode.trim()
            );
        }
    }
    let project_python = if cfg!(target_os = "windows") {
        project.join(".muxiva/venv/Scripts/python.exe")
    } else {
        project.join(".muxiva/venv/bin/python")
    };
    let qwen_ready = Command::new(&project_python)
        .args(["-c", "import websocket"])
        .status()
        .is_ok_and(|status| status.success());
    if qwen_ready {
        println!(
            "[MUXIVA][DOCTOR][PASS] qwen-python dependency=websocket executable={}",
            project_python.display()
        );
    } else {
        *warnings += 1;
        println!(
            "[MUXIVA][DOCTOR][WARN] qwen-python ready=false next=\"./examples/voice-agent/setup.sh\""
        );
    }
    let node = env::var("MUXIVA_NODE").unwrap_or_else(|_| "node".into());
    let node_ready = Command::new(&node)
        .args([
            "--eval",
            "const [M,m]=process.versions.node.split('.').map(Number);process.exit(M>22||(M===22&&m>=19)?0:1)",
        ])
        .status()
        .is_ok_and(|status| status.success());
    let installed_agent_package =
        project.join("node_modules/@piyotahu/muxiva-pi-agent/package.json");
    let agent_source = project.join(".muxiva/agents/muxiva-pi-agent");
    let pi_ready = node_ready
        && installed_agent_package.is_file()
        && project
            .join("node_modules/@muxiva/agent/package.json")
            .is_file()
        && agent_source.join("src/index.ts").is_file()
        && agent_source.join("src/web-search.ts").is_file();
    if pi_ready {
        println!(
            "[MUXIVA][DOCTOR][PASS] pi-typescript-agent node={} version={} release={} source=.muxiva/agents/muxiva-pi-agent dependencies=locked workspace=.muxiva/workspaces/pi-agent capabilities=files,coding,bailian-web-search",
            node,
            tool_version(&node).unwrap_or_else(|| "unknown".into()),
            package_version(&installed_agent_package).unwrap_or_else(|| "unknown".into())
        );
    } else {
        *warnings += 1;
        println!(
            "[MUXIVA][DOCTOR][WARN] pi-typescript-agent ready=false requirement=\"Node.js >=22.19 + external Agent repository + locked npm dependencies\" next=\"./examples/voice-agent/setup.sh\""
        );
    }
    Some(project)
}

struct VoiceCredential {
    label: &'static str,
    environment: &'static str,
    obtain: &'static str,
}

const VOICE_CREDENTIALS: [VoiceCredential; 6] = [
    VoiceCredential {
        label: "Qwen API Key",
        environment: "DASHSCOPE_API_KEY",
        obtain: "https://bailian.console.aliyun.com/ (China Beijing -> API Key)",
    },
    VoiceCredential {
        label: "Qwen Workspace ID",
        environment: "DASHSCOPE_WORKSPACE_ID",
        obtain: "Bailian console top-right Workspace menu; use the same region and workspace as the API Key",
    },
    VoiceCredential {
        label: "Agora App ID",
        environment: "MUXIVA_AGORA_APP_ID",
        obtain: "https://console.agora.io/ -> Projects -> App ID",
    },
    VoiceCredential {
        label: "Agora Channel",
        environment: "MUXIVA_AGORA_CHANNEL",
        obtain: "choose one exact name, for example muxiva-demo; use it for both tokens",
    },
    VoiceCredential {
        label: "Agora Muxiva Bot Token (UID 2001)",
        environment: "MUXIVA_AGORA_BOT_TOKEN",
        obtain: "Agora Token Builder; RTC token for the configured App ID + Channel + UID 2001",
    },
    VoiceCredential {
        label: "Agora Browser Token (UID 1001)",
        environment: "MUXIVA_AGORA_WEB_TOKEN",
        obtain: "Agora Token Builder; RTC token for the configured App ID + Channel + UID 1001",
    },
];

fn dotenv_has_value(project: &Path, key: &str) -> bool {
    fs::read_to_string(project.join(".env"))
        .ok()
        .and_then(|content| {
            content.lines().find_map(|line| {
                let line = line.trim();
                let (candidate, value) = line.split_once('=')?;
                (candidate.trim() == key && !value.trim().trim_matches('"').is_empty())
                    .then_some(())
            })
        })
        .is_some()
}

fn credential_is_set(project: &Path, credential: &VoiceCredential) -> bool {
    env::var_os(credential.environment).is_some_and(|value| !value.is_empty())
        || dotenv_has_value(project, credential.environment)
        || credential.environment == "MUXIVA_AGORA_CHANNEL"
}

fn report_credentials(project: &Path, warnings: &mut usize) {
    let configured = VOICE_CREDENTIALS
        .iter()
        .filter(|credential| credential_is_set(project, credential))
        .count();
    let missing = VOICE_CREDENTIALS.len() - configured;
    if missing == 0 {
        println!(
            "[MUXIVA][DOCTOR][PASS] voice-credentials source=environment-or-project-.env configured={configured}/{} values=redacted",
            VOICE_CREDENTIALS.len()
        );
    } else {
        *warnings += missing;
        println!(
            "[MUXIVA][DOCTOR][WARN] voice-credentials ready=false environment-or-project-.env={configured}/{} missing={missing}",
            VOICE_CREDENTIALS.len()
        );
        for credential in &VOICE_CREDENTIALS {
            if !credential_is_set(project, credential) {
                println!(
                    "[MUXIVA][DOCTOR][MISSING] label=\"{}\" env={} obtain=\"{}\"",
                    credential.label, credential.environment, credential.obtain
                );
            }
        }
        println!(
            "[MUXIVA][DOCTOR][NEXT] edit=\"{}\" source=\".env.example\" then=\"./run.sh\" studio=optional",
            project.join(".env").display()
        );
        println!(
            "[MUXIVA][DOCTOR][NOTE] Save credentials once in {}; the file is Git ignored and loaded automatically by headless run and Studio",
            project.join(".env").display()
        );
    }
    println!(
        "[MUXIVA][DOCTOR][INFO] agora-identities browser_uid=1001 bot_uid=2001 rule=\"same App ID + same Channel; generate one RTC token for each exact UID\""
    );
}

pub(super) fn run(voice: bool, strict: bool) -> Result<(), String> {
    let current = env::current_dir().map_err(|error| error.to_string())?;
    let mut warnings = 0usize;
    println!(
        "[MUXIVA][DOCTOR][PASS] cli version={} platform={}-{}",
        env!("CARGO_PKG_VERSION"),
        env::consts::OS,
        env::consts::ARCH
    );
    if let Some(graph) = discover_studio_graph(&current) {
        println!("[MUXIVA][DOCTOR][PASS] workspace graph={}", graph.display());
    } else {
        println!("[MUXIVA][DOCTOR][INFO] workspace graph=not-found next=\"muxiva init my-agent\"");
    }
    check_tool("Python", &["python3", "python"], &mut warnings);
    check_tool("CMake", &["cmake"], &mut warnings);
    check_tool("C++ compiler", &["c++", "clang++", "g++"], &mut warnings);

    if voice {
        if let Some(project) = inspect_voice_project(&current, &mut warnings) {
            report_credentials(&project, &mut warnings);
        }
    }

    println!("[MUXIVA][DOCTOR][SUMMARY] warnings={warnings} guide={FLAGSHIP_GUIDE}");
    if strict && warnings > 0 {
        Err(format!("doctor found {warnings} warning(s)"))
    } else {
        Ok(())
    }
}
