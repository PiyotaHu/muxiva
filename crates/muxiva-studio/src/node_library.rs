use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
};

use muxiva_core::{
    ConfigMap, ConfigSchema, LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeFactory,
    NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration, NodeRegistry,
    NodeTypeName, PortDescriptor, PortDirection, PortName,
};
use muxiva_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, ErrorCategory, EventData,
    Extensions, Frame, FrameBuffer, FrameDerivation, FrameHeader, FrameId, FramePayload, FrameType,
    Lineage, Metadata, MuxivaError, NamespacedName, NodeId, PcmSampleFormat, SchemaVersion,
    SequenceId, SignalData, StreamId, TextData, Timestamp, TraceId, TransformOrigin,
};
use serde::{Deserialize, Serialize};

const FORMAT: &str = "muxiva.node/v1";
const PROVIDER_FORMAT: &str = "muxiva.provider/v1";
const PROVIDER_CONFIG_FORMAT: &str = "muxiva.providers/v1";
const MAX_CODE_BYTES: usize = 512 * 1024;
const MAX_PROVIDER_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_HOST_RESPONSE_BYTES: usize = 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
static NEXT_FRAME: AtomicU64 = AtomicU64::new(0);

const PYTHON_HOST: &str = r#"
import importlib.util, inspect, json, sys, types

class TextFrame:
    def __init__(self, text, sequence=0): self.text, self.sequence = text, sequence

class AudioFrame:
    def __init__(self, data, sample_rate_hz, channels=1, sequence=0):
        self.data, self.sample_rate_hz = bytes(data), sample_rate_hz
        self.channels, self.sequence = channels, sequence

class EventFrame:
    def __init__(self, topic, payload="", source="python.node", schema_version=1, sequence=0, **_):
        self.topic, self.payload, self.source = topic, payload, source
        self.schema_version, self.sequence = schema_version, sequence

class SignalFrame:
    def __init__(self, name, payload="", source="runtime.node", schema_version=1, sequence=0, **_):
        self.name, self.payload, self.source = name, payload, source
        self.schema_version, self.sequence = schema_version, sequence

shim = types.ModuleType("muxiva")
shim.TextFrame = TextFrame
shim.AudioFrame = AudioFrame
shim.EventFrame = EventFrame
shim.SignalFrame = SignalFrame

class NodeContext:
    def __init__(self, node_id, input_port, config, streaming=False):
        self.node_id, self.input_port, self.config = node_id, input_port, config
        self.streaming = streaming
        self.emissions, self.signals, self.events = [], [], []
    def emit(self, port, frame):
        emission = {"port":port, "frame":encode_frame(frame)}
        if self.streaming:
            print(json.dumps({"kind":"emission", **emission}), flush=True)
        else:
            self.emissions.append(emission)
    def emit_signal(self, name, payload=None):
        value = {"name":name, "payload":payload}
        if self.streaming: print(json.dumps({"kind":"signal", **value}), flush=True)
        else: self.signals.append(value)
    def publish_event(self, topic, payload=None):
        value = {"topic":topic, "payload":payload}
        if self.streaming: print(json.dumps({"kind":"event", **value}), flush=True)
        else: self.events.append(value)
    def __str__(self): return self.input_port or ""
    def __eq__(self, other): return self.input_port == other

shim.NodeContext = NodeContext
sys.modules["muxiva"] = shim

source, entrypoint, config_json = sys.argv[1:4]
spec = importlib.util.spec_from_file_location("muxiva_project_node", source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
symbol = entrypoint.split(":", 1)[-1]
constructor = getattr(module, symbol)
config = json.loads(config_json)
try:
    node = constructor(config)
except TypeError:
    node = constructor()

def invoke(name, *args):
    callback = getattr(node, name, None)
    if callback is None: return None
    count = len(inspect.signature(callback).parameters)
    return callback(*args[:count])

def decode_frame(value):
    if value is None: return None
    if value.get("kind") == "text": return TextFrame(value["text"], value.get("sequence", 0))
    if value.get("kind") == "audio": return AudioFrame(bytes.fromhex(value["pcm_hex"]), value["sample_rate_hz"], value["channels"], value.get("sequence", 0))
    if value.get("kind") == "signal": return SignalFrame(value["name"], value.get("payload", ""), value.get("source", "runtime.node"), value.get("schema_version", 1), value.get("sequence", 0))
    raise ValueError("Studio Python Host received an unsupported Frame")

def encode_frame(value):
    if isinstance(value, TextFrame): return {"kind":"text", "text":value.text, "sequence":value.sequence}
    if isinstance(value, AudioFrame): return {"kind":"audio", "pcm_hex":value.data.hex(), "sample_rate_hz":value.sample_rate_hz, "channels":value.channels, "sequence":value.sequence}
    if isinstance(value, EventFrame): return {"kind":"event", "topic":value.topic, "payload":value.payload, "source":value.source, "schema_version":value.schema_version, "sequence":value.sequence}
    if isinstance(value, dict) and value.get("kind") == "text": return value
    raise ValueError("Python Node emitted an unsupported Frame")

print(json.dumps({"ready": True}), flush=True)
for line in sys.stdin:
    try:
        command = json.loads(line)
        op = command["op"]
        if op == "process":
            frame = decode_frame(command.get("frame"))
            ctx = NodeContext(command["node_id"], command.get("input_port"), config, streaming=True)
            result = invoke("on_process", frame, ctx)
            if result is not None:
                values = result if isinstance(result, dict) else {command["default_output"]: result}
                for port, frames in values.items():
                    if not isinstance(frames, list): frames = [frames]
                    for item in frames: ctx.emit(port, item)
            response = {"ok": True, "signals":ctx.signals, "events":ctx.events}
        elif op == "signal":
            signal = decode_frame(command["signal"])
            ctx = NodeContext(command["node_id"], command.get("input_port"), config, streaming=False)
            invoke("on_signal", signal, ctx)
            response = {"ok": True, "signals":ctx.signals, "events":ctx.events}
        elif op == "prepare": invoke("on_prepare", NodeContext(command["node_id"], None, config)); response = {"ok": True}
        elif op == "finish": invoke("on_finish", NodeContext(command["node_id"], None, config)); response = {"ok": True}
        elif op == "abort": invoke("on_abort", command.get("reason", "aborted"), NodeContext(command["node_id"], None, config)); response = {"ok": True}
        elif op == "close": print(json.dumps({"ok": True}), flush=True); break
        else: raise ValueError("unknown Host operation")
    except Exception as error:
        response = {"ok": False, "error": f"{type(error).__name__}: {error}"}
    print(json.dumps(response), flush=True)
"#;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodePortManifest {
    pub name: String,
    pub direction: String,
    pub frame_type: String,
    #[serde(default = "empty_json_object")]
    pub schema: serde_json::Value,
}

fn empty_json_object() -> serde_json::Value {
    serde_json::json!({})
}

fn default_category() -> String {
    "utility".to_owned()
}

fn default_capability() -> String {
    "custom".to_owned()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodePackageManifest {
    pub format: String,
    pub package_id: String,
    pub display_name: String,
    pub node_type: String,
    pub language: String,
    pub factory_version: String,
    pub kind: String,
    pub entrypoint: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default = "default_capability")]
    pub capability: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub ports: Vec<NodePortManifest>,
    pub config_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<ConnectionManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderSdkManifest {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderManifest {
    pub format: String,
    pub provider_id: String,
    pub display_name: String,
    pub category: String,
    pub summary: String,
    pub vendor: String,
    pub homepage: String,
    pub documentation: String,
    pub license: String,
    pub sdk: ProviderSdkManifest,
    #[serde(default)]
    pub connections: Vec<ConnectionManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConnectionManifest {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub fields: Vec<ConnectionFieldManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConnectionFieldManifest {
    pub name: String,
    pub label: String,
    pub environment: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub required: bool,
    /// Explicit opt-in for short-lived values needed by a project browser app.
    #[serde(default)]
    pub client_exposed: bool,
    #[serde(default)]
    pub default: String,
    /// Short in-product explanation shown next to the field.
    #[serde(default)]
    pub help: String,
    /// Official console or documentation page where the value is obtained.
    #[serde(default)]
    pub acquire_url: String,
}

struct SecretBytes(Vec<u8>);

impl SecretBytes {
    fn replace(&mut self, value: &str) {
        self.0.fill(0);
        self.0.clear();
        self.0.extend_from_slice(value.as_bytes());
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Clone)]
pub struct ConnectionStore {
    manifests: Arc<Vec<ConnectionManifest>>,
    values: Arc<Mutex<BTreeMap<(String, String), SecretBytes>>>,
    env_path: Arc<PathBuf>,
}

impl ConnectionStore {
    pub fn load(graph: &Path) -> Result<Self, String> {
        let mut manifests = BTreeMap::<String, ConnectionManifest>::new();
        for package in list(graph).map_err(|error| error.to_string())? {
            if let Some(provider) = package.provider_manifest {
                for connection in provider.connections {
                    insert_connection(&mut manifests, connection)?;
                }
            }
            if let Some(connection) = package.resolved_connection {
                insert_connection(&mut manifests, connection)?;
            }
        }
        let manifests = manifests.into_values().collect::<Vec<_>>();
        let env_path = graph
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".env");
        let file_values = read_dotenv(&env_path)?;
        let mut values = BTreeMap::new();
        for connection in &manifests {
            for field in &connection.fields {
                let value = std::env::var(&field.environment)
                    .ok()
                    .or_else(|| file_values.get(&field.environment).cloned())
                    .unwrap_or_else(|| field.default.clone());
                if !value.is_empty() {
                    values.insert(
                        (connection.id.clone(), field.name.clone()),
                        SecretBytes(value.into_bytes()),
                    );
                }
            }
        }
        let store = Self {
            manifests: Arc::new(manifests),
            values: Arc::new(Mutex::new(values)),
            env_path: Arc::new(env_path),
        };
        store.apply_to_process_environment();
        Ok(store)
    }

    pub fn status_json(&self) -> serde_json::Value {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        serde_json::json!({
            "connections": self.manifests.iter().map(|connection| {
                let fields = connection.fields.iter().map(|field| {
                    let value = values.get(&(connection.id.clone(), field.name.clone()));
                    serde_json::json!({
                        "name": field.name,
                        "label": field.label,
                        "secret": field.secret,
                        "required": field.required,
                        "environment": field.environment,
                        "help": field.help,
                        "acquire_url": field.acquire_url,
                        "set": value.is_some_and(|value| !value.0.is_empty()),
                        "value": if field.secret { "".into() } else { value.map(|value| String::from_utf8_lossy(&value.0).into_owned()).unwrap_or_default() },
                    })
                }).collect::<Vec<_>>();
                let configured = connection.fields.iter().all(|field| !field.required || values.get(&(connection.id.clone(), field.name.clone())).is_some_and(|value| !value.0.is_empty()));
                serde_json::json!({
                    "id": connection.id,
                    "display_name": connection.display_name,
                    "description": connection.description,
                    "configured": configured,
                    "fields": fields,
                })
            }).collect::<Vec<_>>(),
            "storage": "project-.env",
        })
    }

    pub fn missing_required_for(
        &self,
        connection_ids: &std::collections::BTreeSet<String>,
    ) -> Vec<String> {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.manifests
            .iter()
            .filter(|connection| connection_ids.contains(&connection.id))
            .flat_map(|connection| {
                connection.fields.iter().filter_map(|field| {
                    let configured = values
                        .get(&(connection.id.clone(), field.name.clone()))
                        .is_some_and(|value| !value.0.is_empty());
                    (field.required && !configured).then(|| {
                        format!(
                            "{} / {} ({})",
                            connection.display_name, field.label, field.environment
                        )
                    })
                })
            })
            .collect()
    }

    pub fn client_json(&self) -> serde_json::Value {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let connections = self
            .manifests
            .iter()
            .map(|connection| {
                let fields = connection
                    .fields
                    .iter()
                    .filter(|field| field.client_exposed)
                    .filter_map(|field| {
                        values
                            .get(&(connection.id.clone(), field.name.clone()))
                            .map(|value| {
                                (
                                    field.name.clone(),
                                    serde_json::Value::String(
                                        String::from_utf8_lossy(&value.0).into_owned(),
                                    ),
                                )
                            })
                    })
                    .collect::<serde_json::Map<_, _>>();
                (connection.id.clone(), serde_json::Value::Object(fields))
            })
            .filter(|(_, value)| value.as_object().is_some_and(|fields| !fields.is_empty()))
            .collect::<serde_json::Map<_, _>>();
        serde_json::Value::Object(connections)
    }

    pub fn update_json(&self, input: &str) -> Result<(), String> {
        let root: serde_json::Value = serde_json::from_str(input)
            .map_err(|_| "connection configuration must be valid JSON")?;
        let updates = root
            .get("connections")
            .and_then(serde_json::Value::as_object)
            .ok_or("connections must be an object")?;
        let mut values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (connection_id, update) in updates {
            let manifest = self
                .manifests
                .iter()
                .find(|connection| connection.id == *connection_id)
                .ok_or_else(|| format!("unknown connection `{connection_id}`"))?;
            let update = update
                .as_object()
                .ok_or_else(|| format!("connection `{connection_id}` must be an object"))?;
            for (name, value) in update {
                let field = manifest
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .ok_or_else(|| format!("unknown field `{connection_id}.{name}`"))?;
                let value = value
                    .as_str()
                    .ok_or_else(|| format!("field `{connection_id}.{name}` must be a string"))?;
                if value.len() > 16 * 1024 {
                    return Err(format!("field `{connection_id}.{name}` exceeds 16 KiB"));
                }
                if value.contains(['\0', '\n', '\r']) {
                    return Err(format!(
                        "field `{connection_id}.{name}` cannot contain NUL or a line break"
                    ));
                }
                if field.secret && value.is_empty() {
                    continue;
                }
                values
                    .entry((connection_id.clone(), name.clone()))
                    .or_insert_with(|| SecretBytes(Vec::new()))
                    .replace(value.trim());
            }
        }
        drop(values);
        self.persist_dotenv()?;
        self.apply_to_process_environment();
        Ok(())
    }

    fn persist_dotenv(&self) -> Result<(), String> {
        let mut output = read_dotenv(&self.env_path)?;
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for connection in self.manifests.iter() {
            for field in &connection.fields {
                if let Some(value) = values.get(&(connection.id.clone(), field.name.clone())) {
                    output.insert(
                        field.environment.clone(),
                        String::from_utf8_lossy(&value.0).into_owned(),
                    );
                }
            }
        }
        write_dotenv(&self.env_path, &output)
    }

    fn apply_to_process_environment(&self) {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for connection in self.manifests.iter() {
            for field in &connection.fields {
                if let Some(value) = values.get(&(connection.id.clone(), field.name.clone())) {
                    std::env::set_var(
                        &field.environment,
                        String::from_utf8_lossy(&value.0).as_ref(),
                    );
                }
            }
        }
    }

    fn apply_to_command(&self, command: &mut Command, connection: Option<&ConnectionManifest>) {
        let Some(connection) = connection else {
            return;
        };
        let values = self
            .values
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for field in &connection.fields {
            if let Some(value) = values.get(&(connection.id.clone(), field.name.clone())) {
                command.env(
                    &field.environment,
                    String::from_utf8_lossy(&value.0).as_ref(),
                );
            }
        }
    }
}

fn read_dotenv(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    let mut values = BTreeMap::new();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, raw) = line
            .split_once('=')
            .ok_or_else(|| format!("{}:{} must use KEY=value", path.display(), index + 1))?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(format!(
                "{}:{} has an invalid key",
                path.display(),
                index + 1
            ));
        }
        let raw = raw.trim();
        let value = if raw.starts_with('"') {
            serde_json::from_str::<String>(raw).map_err(|_| {
                format!(
                    "{}:{} has an invalid quoted value",
                    path.display(),
                    index + 1
                )
            })?
        } else {
            raw.to_owned()
        };
        values.insert(key.to_owned(), value);
    }
    Ok(values)
}

fn write_dotenv(path: &Path, values: &BTreeMap<String, String>) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let temporary = parent.join(format!(
        ".env.{}.{}.tmp",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), String> {
        let mut file = options
            .open(&temporary)
            .map_err(|error| format!("failed to create {}: {error}", temporary.display()))?;
        writeln!(
            file,
            "# Local Muxiva provider credentials. Never commit this file."
        )
        .map_err(|error| error.to_string())?;
        for (key, value) in values {
            let quoted = serde_json::to_string(value).map_err(|error| error.to_string())?;
            writeln!(file, "{key}={quoted}").map_err(|error| error.to_string())?;
        }
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("failed to replace {}: {error}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn insert_connection(
    manifests: &mut BTreeMap<String, ConnectionManifest>,
    connection: ConnectionManifest,
) -> Result<(), String> {
    match manifests.get(&connection.id) {
        Some(existing) if existing != &connection => Err(format!(
            "Provider packages declare conflicting connection `{}`",
            connection.id
        )),
        Some(_) => Ok(()),
        None => {
            manifests.insert(connection.id.clone(), connection);
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodePackage {
    #[serde(flatten)]
    pub manifest: NodePackageManifest,
    pub code: String,
    pub runtime_available: bool,
    #[serde(default = "project_origin")]
    pub origin: String,
    #[serde(default = "default_editable")]
    pub editable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_manifest: Option<ProviderManifest>,
    #[serde(skip)]
    source_directory: PathBuf,
    #[serde(skip)]
    resolved_connection: Option<ConnectionManifest>,
}

impl NodePackage {
    pub fn resolved_connection_id(&self) -> Option<&str> {
        self.resolved_connection
            .as_ref()
            .map(|connection| connection.id.as_str())
    }
}

fn project_origin() -> String {
    "project".to_owned()
}

const fn default_editable() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderRootsConfig {
    format: String,
    roots: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum SaveError {
    Invalid(String),
    Io(io::Error),
}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn list(graph: &Path) -> io::Result<Vec<NodePackage>> {
    let mut packages = Vec::new();
    let mut identities = BTreeSet::new();
    let mut package_ids = BTreeSet::new();
    for (root, origin, editable) in package_roots(graph)? {
        let mut directories = Vec::new();
        if let Err(error) = collect_node_directories(&root, &mut directories, 0) {
            if error.kind() == io::ErrorKind::NotFound && editable {
                continue;
            }
            return Err(error);
        }
        if directories.len() > 1024 {
            return Err(invalid_data(
                "a Provider Root may contain at most 1024 Node packages",
            ));
        }
        directories.sort();
        for directory in directories {
            let package = read_package(directory, &root, graph, origin.clone(), editable)?;
            let identity = (
                package.manifest.node_type.clone(),
                package.manifest.language.clone(),
                package.manifest.factory_version.clone(),
            );
            if !package_ids.insert(package.manifest.package_id.clone()) {
                return Err(invalid_data(format!(
                    "duplicate Node package_id `{}` across project and Provider Roots",
                    package.manifest.package_id
                )));
            }
            if !identities.insert(identity) {
                return Err(invalid_data(format!(
                    "duplicate Node Factory identity for `{}`",
                    package.manifest.node_type
                )));
            }
            packages.push(package);
        }
    }
    Ok(packages)
}

pub fn provider_catalog(graph: &Path) -> io::Result<Vec<ProviderManifest>> {
    let mut providers = BTreeMap::<String, ProviderManifest>::new();
    for package in list(graph)? {
        let Some(provider) = package.provider_manifest else {
            continue;
        };
        match providers.get(&provider.provider_id) {
            Some(existing) if existing != &provider => {
                return Err(invalid_data(format!(
                    "conflicting Provider Manifests for `{}`",
                    provider.provider_id
                )))
            }
            Some(_) => {}
            None => {
                providers.insert(provider.provider_id.clone(), provider);
            }
        }
    }
    Ok(providers.into_values().collect())
}

fn collect_node_directories(
    directory: &Path,
    output: &mut Vec<PathBuf>,
    depth: usize,
) -> io::Result<()> {
    if depth > 12 {
        return Err(invalid_data("Provider Root nesting exceeds 12 directories"));
    }
    if directory.join("muxiva.node.json").is_file() {
        output.push(directory.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            collect_node_directories(&entry.path(), output, depth + 1)?;
        }
    }
    Ok(())
}

pub fn register_project_nodes_with_connections(
    graph: &Path,
    registry: &mut NodeRegistry,
    connections: ConnectionStore,
) -> Result<(), String> {
    for package in list(graph).map_err(|error| error.to_string())? {
        let registration = match package.manifest.language.as_str() {
            "python" if python_host_supported(graph, &package) => {
                python_registration(graph, &package, connections.clone())?
            }
            "cpp" if cpp_host_supported(graph, &package.manifest) => {
                cpp_registration(graph, &package.manifest)?
            }
            _ => continue,
        };
        registry
            .register(registration)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn save(graph: &Path, input: &str) -> Result<NodePackage, SaveError> {
    let mut package: NodePackage = serde_json::from_str(input)
        .map_err(|error| SaveError::Invalid(format!("invalid Node package JSON: {error}")))?;
    validate(&package)?;
    package.manifest.format = FORMAT.to_owned();
    package.runtime_available = false;
    package.origin = project_origin();
    package.editable = true;
    let directory = library_root(graph).join(&package.manifest.package_id);
    package.source_directory = directory.clone();
    fs::create_dir_all(&directory)?;
    atomic_write(
        &directory.join("muxiva.node.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&package.manifest)
                .map_err(|error| SaveError::Invalid(error.to_string()))?
        )
        .as_bytes(),
    )?;
    atomic_write(
        &directory.join(source_filename(&package.manifest.language)),
        package.code.as_bytes(),
    )?;
    Ok(package)
}

fn validate(package: &NodePackage) -> Result<(), SaveError> {
    let manifest = &package.manifest;
    if !valid_package_id(&manifest.package_id) {
        return Err(SaveError::Invalid(
            "package_id must use 1-64 lowercase letters, digits, '-' or '_'".into(),
        ));
    }
    if manifest.display_name.trim().is_empty() || manifest.display_name.len() > 100 {
        return Err(SaveError::Invalid(
            "display_name must contain 1-100 characters".into(),
        ));
    }
    NodeTypeName::new(manifest.node_type.clone())
        .map_err(|error| SaveError::Invalid(format!("invalid node_type: {error}")))?;
    NodeFactoryVersion::new(manifest.factory_version.clone())
        .map_err(|error| SaveError::Invalid(format!("invalid factory_version: {error}")))?;
    NodeLanguage::parse(&manifest.language).ok_or_else(|| {
        SaveError::Invalid("language must be rust, cpp, python, or typescript".into())
    })?;
    if !matches!(manifest.kind.as_str(), "source" | "transform" | "sink") {
        return Err(SaveError::Invalid(
            "kind must be source, transform, or sink".into(),
        ));
    }
    if !valid_category(&manifest.category) {
        return Err(SaveError::Invalid(
            "category must be transport, algorithm, media, control, or utility".into(),
        ));
    }
    if !valid_capability(&manifest.capability) {
        return Err(SaveError::Invalid(
            "capability must use lowercase dot-separated identifiers".into(),
        ));
    }
    if manifest.summary.len() > 280 {
        return Err(SaveError::Invalid(
            "summary may contain at most 280 characters".into(),
        ));
    }
    if let Some(provider_id) = &manifest.provider_id {
        if !valid_package_id(provider_id) {
            return Err(SaveError::Invalid(
                "provider_id must be a filesystem-safe identifier".into(),
            ));
        }
    }
    if let Some(connection_id) = &manifest.connection_id {
        if !valid_package_id(connection_id) {
            return Err(SaveError::Invalid(
                "connection_id must be a filesystem-safe identifier".into(),
            ));
        }
    }
    if manifest.connection_id.is_some() && manifest.connection.is_some() {
        return Err(SaveError::Invalid(
            "connection_id and inline connection are mutually exclusive".into(),
        ));
    }
    if manifest.ports.len() > 64 {
        return Err(SaveError::Invalid(
            "a Node may declare at most 64 Ports".into(),
        ));
    }
    for port in &manifest.ports {
        PortName::new(port.name.clone())
            .map_err(|error| SaveError::Invalid(format!("invalid Port name: {error}")))?;
        if !matches!(port.direction.as_str(), "input" | "output") {
            return Err(SaveError::Invalid(
                "Port direction must be input or output".into(),
            ));
        }
        if !matches!(
            port.frame_type.as_str(),
            "audio" | "video" | "text" | "byte" | "signal" | "event"
        ) {
            return Err(SaveError::Invalid(format!(
                "unsupported Port frame_type `{}`",
                port.frame_type
            )));
        }
        if !port.schema.is_object() {
            return Err(SaveError::Invalid(format!(
                "Port `{}` schema must be a JSON object",
                port.name
            )));
        }
    }
    if !manifest.config_schema.is_object() {
        return Err(SaveError::Invalid(
            "config_schema must be a JSON object".into(),
        ));
    }
    if let Some(connection) = &manifest.connection {
        validate_connection(connection)?;
    }
    if package.code.is_empty() || package.code.len() > MAX_CODE_BYTES {
        return Err(SaveError::Invalid(
            "code must contain 1 byte through 512 KiB".into(),
        ));
    }
    Ok(())
}

fn validate_connection(connection: &ConnectionManifest) -> Result<(), SaveError> {
    if !valid_package_id(&connection.id) {
        return Err(SaveError::Invalid(
            "connection id must use 1-64 lowercase letters, digits, '-' or '_'".into(),
        ));
    }
    if connection.display_name.trim().is_empty() || connection.fields.is_empty() {
        return Err(SaveError::Invalid(
            "connection requires a display name and at least one field".into(),
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for field in &connection.fields {
        if !valid_package_id(&field.name) || !names.insert(&field.name) {
            return Err(SaveError::Invalid(
                "connection field names must be unique stable identifiers".into(),
            ));
        }
        if field.label.trim().is_empty()
            || field.environment.is_empty()
            || !field
                .environment
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(SaveError::Invalid(
                "connection fields require labels and uppercase environment names".into(),
            ));
        }
    }
    Ok(())
}

fn valid_package_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

fn valid_category(value: &str) -> bool {
    matches!(
        value,
        "transport" | "algorithm" | "media" | "control" | "utility"
    )
}

fn valid_capability(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .split('.')
            .all(|part| !part.is_empty() && valid_package_id(part))
}

fn library_root(graph: &Path) -> PathBuf {
    graph
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".muxiva")
        .join("nodes")
}

fn package_roots(graph: &Path) -> io::Result<Vec<(PathBuf, String, bool)>> {
    let project_root = graph.parent().unwrap_or_else(|| Path::new("."));
    let muxiva_root = project_root.join(".muxiva");
    let mut roots = vec![(muxiva_root.join("nodes"), project_origin(), true)];
    let config_path = muxiva_root.join("providers.json");
    let metadata = match fs::metadata(&config_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(roots),
        Err(error) => return Err(error),
    };
    if metadata.len() > MAX_PROVIDER_CONFIG_BYTES {
        return Err(invalid_data("Provider Root config exceeds 64 KiB"));
    }
    let config: ProviderRootsConfig = serde_json::from_str(&fs::read_to_string(&config_path)?)
        .map_err(|error| invalid_data(format!("invalid Provider Root config: {error}")))?;
    if config.format != PROVIDER_CONFIG_FORMAT {
        return Err(invalid_data(format!(
            "unsupported Provider Root format `{}`",
            config.format
        )));
    }
    if config.roots.len() > 32 {
        return Err(invalid_data("at most 32 Provider Roots may be configured"));
    }
    let mut seen = BTreeSet::new();
    for relative in config.roots {
        if relative.is_absolute() {
            return Err(invalid_data(
                "Provider Roots must be relative to the .muxiva directory",
            ));
        }
        let resolved = muxiva_root
            .join(&relative)
            .canonicalize()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "cannot resolve Provider Root `{}`: {error}",
                        relative.display()
                    ),
                )
            })?;
        if !resolved.is_dir() {
            return Err(invalid_data(format!(
                "Provider Root is not a directory: {}",
                relative.display()
            )));
        }
        if !seen.insert(resolved.clone()) {
            return Err(invalid_data(format!(
                "duplicate Provider Root: {}",
                relative.display()
            )));
        }
        roots.push((resolved, "provider".to_owned(), false));
    }
    Ok(roots)
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn source_filename(language: &str) -> &'static str {
    match language {
        "python" => "node.py",
        "typescript" => "node.ts",
        "rust" => "node.rs",
        "cpp" => "node.cpp",
        _ => "node.txt",
    }
}

fn read_package(
    directory: PathBuf,
    provider_root: &Path,
    graph: &Path,
    origin: String,
    editable: bool,
) -> io::Result<NodePackage> {
    let mut manifest: NodePackageManifest =
        serde_json::from_str(&fs::read_to_string(directory.join("muxiva.node.json"))?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let legacy_node_type = manifest.node_type.clone();
    manifest.node_type = muxiva_graph_json::canonical_node_type(&legacy_node_type).to_owned();
    if legacy_node_type == "provider.agora.audio_source" {
        manifest.factory_version = "1.1.0".to_owned();
    }
    let provider_manifest = read_nearest_provider_manifest(&directory, provider_root)?;
    if let (Some(expected), Some(provider)) = (&manifest.provider_id, &provider_manifest) {
        if expected != &provider.provider_id {
            return Err(invalid_data(format!(
                "Node `{}` declares provider `{expected}` but is owned by `{}`",
                manifest.package_id, provider.provider_id
            )));
        }
    }
    let resolved_connection = match (&manifest.connection_id, &manifest.connection) {
        (Some(_), Some(_)) => {
            return Err(invalid_data(format!(
                "Node `{}` cannot declare both connection_id and an inline connection",
                manifest.package_id
            )))
        }
        (Some(connection_id), None) => {
            let provider = provider_manifest.as_ref().ok_or_else(|| {
                invalid_data(format!(
                    "Node `{}` references connection `{connection_id}` without a Provider Manifest",
                    manifest.package_id
                ))
            })?;
            Some(
                provider
                    .connections
                    .iter()
                    .find(|connection| connection.id == *connection_id)
                    .cloned()
                    .ok_or_else(|| {
                        invalid_data(format!(
                            "Provider `{}` does not declare connection `{connection_id}`",
                            provider.provider_id
                        ))
                    })?,
            )
        }
        (None, connection) => connection.clone(),
    };
    let code = fs::read_to_string(directory.join(source_filename(&manifest.language)))?;
    let runtime_available = python_host_supported_manifest(graph, &manifest)
        || (manifest.language == "cpp" && cpp_artifact_path(graph, &manifest.package_id).is_file());
    Ok(NodePackage {
        manifest,
        code,
        runtime_available,
        origin,
        editable,
        provider_manifest,
        source_directory: directory,
        resolved_connection,
    })
}

fn read_nearest_provider_manifest(
    directory: &Path,
    provider_root: &Path,
) -> io::Result<Option<ProviderManifest>> {
    let mut current = Some(directory);
    while let Some(candidate) = current {
        let path = candidate.join("muxiva.provider.json");
        if path.is_file() {
            let provider: ProviderManifest = serde_json::from_str(&fs::read_to_string(path)?)
                .map_err(|error| invalid_data(format!("invalid Provider Manifest: {error}")))?;
            validate_provider_manifest(&provider)?;
            return Ok(Some(provider));
        }
        if candidate == provider_root {
            break;
        }
        current = candidate.parent();
    }
    Ok(None)
}

fn validate_provider_manifest(provider: &ProviderManifest) -> io::Result<()> {
    if provider.format != PROVIDER_FORMAT {
        return Err(invalid_data(format!(
            "unsupported Provider Manifest format `{}`",
            provider.format
        )));
    }
    if !valid_package_id(&provider.provider_id) {
        return Err(invalid_data(
            "Provider ID must be a filesystem-safe identifier",
        ));
    }
    if !valid_category(&provider.category) {
        return Err(invalid_data(format!(
            "unsupported Provider category `{}`",
            provider.category
        )));
    }
    if provider.display_name.trim().is_empty()
        || provider.summary.trim().is_empty()
        || provider.vendor.trim().is_empty()
    {
        return Err(invalid_data(
            "Provider display_name, summary and vendor must not be empty",
        ));
    }
    let mut connection_ids = BTreeSet::new();
    for connection in &provider.connections {
        validate_connection(connection).map_err(|error| match error {
            SaveError::Invalid(message) => invalid_data(message),
            SaveError::Io(error) => error,
        })?;
        if !connection_ids.insert(&connection.id) {
            return Err(invalid_data(format!(
                "duplicate Provider connection `{}`",
                connection.id
            )));
        }
    }
    Ok(())
}

fn cpp_registration(
    graph: &Path,
    manifest: &NodePackageManifest,
) -> Result<NodeRegistration, String> {
    let path = cpp_artifact_path(graph, &manifest.package_id);
    let mut registration = muxiva_ffi::load_cpp_multimodal_node_pack(&path)?;
    validate_cpp_registration(manifest, &registration)?;
    if registration.descriptor().node_type().as_str() != manifest.node_type {
        registration = muxiva_ffi::load_cpp_multimodal_node_pack_as(
            &path,
            NodeTypeName::new(manifest.node_type.clone()).map_err(|error| error.to_string())?,
        )?;
    }
    Ok(registration)
}

fn validate_cpp_registration(
    manifest: &NodePackageManifest,
    registration: &NodeRegistration,
) -> Result<(), String> {
    let descriptor = registration.descriptor();
    let expected_kind = match manifest.kind.as_str() {
        "source" => NodeKind::Source,
        "transform" => NodeKind::Transform,
        "sink" => NodeKind::Sink,
        _ => return Err("invalid C++ Node kind".into()),
    };
    if muxiva_graph_json::canonical_node_type(descriptor.node_type().as_str()) == manifest.node_type
        && registration.version().as_str() != manifest.factory_version
    {
        return Err(format!(
            "C++ artifact `{}` is version {}, but its Manifest requires {}; rebuild the Node pack",
            manifest.package_id,
            registration.version().as_str(),
            manifest.factory_version
        ));
    }
    if registration.language() != NodeLanguage::Cpp
        || muxiva_graph_json::canonical_node_type(descriptor.node_type().as_str())
            != manifest.node_type
        || registration.version().as_str() != manifest.factory_version
        || descriptor.kind() != expected_kind
        || descriptor.ports().len() != manifest.ports.len()
    {
        return Err(format!(
            "C++ artifact identity does not match Manifest `{}`",
            manifest.package_id
        ));
    }
    for (actual, expected) in descriptor.ports().iter().zip(&manifest.ports) {
        let expected_direction = match expected.direction.as_str() {
            "input" => PortDirection::Input,
            "output" => PortDirection::Output,
            _ => return Err("invalid C++ Port direction".into()),
        };
        if actual.name().as_str() != expected.name
            || actual.direction() != expected_direction
            || actual.frame_type() != parse_frame_type(&expected.frame_type)?
        {
            return Err(format!(
                "C++ artifact Port shape does not match Manifest `{}`",
                manifest.package_id
            ));
        }
    }
    Ok(())
}

fn cpp_host_supported(graph: &Path, manifest: &NodePackageManifest) -> bool {
    manifest.language == "cpp" && cpp_artifact_path(graph, &manifest.package_id).is_file()
}

fn cpp_artifact_path(graph: &Path, package_id: &str) -> PathBuf {
    native_node_root(graph)
        .join(package_id)
        .join(native_library_filename())
}

fn native_node_root(graph: &Path) -> PathBuf {
    std::env::var_os("MUXIVA_NATIVE_NODE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            graph
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(".muxiva/native")
        })
}

fn native_library_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "libmuxiva_node_pack.dylib"
    } else if cfg!(target_os = "windows") {
        "muxiva_node_pack.dll"
    } else {
        "libmuxiva_node_pack.so"
    }
}

fn python_registration(
    graph: &Path,
    package: &NodePackage,
    connections: ConnectionStore,
) -> Result<NodeRegistration, String> {
    let manifest = &package.manifest;
    let template = NodeId::new(format!("template-{}", manifest.node_type))
        .map_err(|error| error.to_string())?;
    let ports = manifest
        .ports
        .iter()
        .map(|port| {
            Ok(PortDescriptor::new(
                template.clone(),
                PortName::new(port.name.clone()).map_err(|error| error.to_string())?,
                match port.direction.as_str() {
                    "input" => PortDirection::Input,
                    "output" => PortDirection::Output,
                    _ => return Err("invalid project Port direction".into()),
                },
                parse_frame_type(&port.frame_type)?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let kind = match manifest.kind.as_str() {
        "source" => NodeKind::Source,
        "transform" => NodeKind::Transform,
        "sink" => NodeKind::Sink,
        _ => return Err("invalid project Node kind".into()),
    };
    let descriptor = NodeDescriptor::new(
        template,
        NodeTypeName::new(manifest.node_type.clone()).map_err(|error| error.to_string())?,
        kind,
        ports,
        ConfigSchema::new(muxiva_graph_json::value_from_json(&manifest.config_schema)?),
        LifecycleCapabilities::new(true, true, true, true),
    );
    let source = package.source_directory.join(source_filename("python"));
    let default_output = manifest
        .ports
        .iter()
        .find(|port| port.direction == "output")
        .map(|port| port.name.clone());
    Ok(NodeRegistration::new(
        NodeLanguage::Python,
        descriptor,
        NodeFactoryVersion::new(manifest.factory_version.clone())
            .map_err(|error| error.to_string())?,
        Arc::new(PythonDevFactory {
            executable: python_executable(graph),
            source,
            entrypoint: manifest.entrypoint.clone(),
            default_output,
            connection: package.resolved_connection.clone(),
            connections,
        }),
    ))
}

fn parse_frame_type(value: &str) -> Result<FrameType, String> {
    match value {
        "audio" => Ok(FrameType::Audio),
        "video" => Ok(FrameType::Video),
        "text" => Ok(FrameType::Text),
        "byte" => Ok(FrameType::Byte),
        "signal" => Ok(FrameType::Signal),
        "event" => Ok(FrameType::Event),
        _ => Err(format!("unsupported Frame type `{value}`")),
    }
}

fn python_executable(graph: &Path) -> PathBuf {
    if let Some(executable) = std::env::var_os("MUXIVA_PYTHON") {
        return PathBuf::from(executable);
    }
    let muxiva_root = graph
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".muxiva/venv");
    let project_python = if cfg!(target_os = "windows") {
        muxiva_root.join("Scripts/python.exe")
    } else {
        muxiva_root.join("bin/python")
    };
    if project_python.is_file() {
        project_python
    } else {
        PathBuf::from("python3")
    }
}

fn python_available(graph: &Path) -> bool {
    Command::new(python_executable(graph))
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn python_host_supported(graph: &Path, package: &NodePackage) -> bool {
    python_host_supported_manifest(graph, &package.manifest)
}

fn python_host_supported_manifest(graph: &Path, manifest: &NodePackageManifest) -> bool {
    manifest.language == "python"
        && manifest
            .ports
            .iter()
            .all(|port| match port.direction.as_str() {
                "input" => matches!(port.frame_type.as_str(), "text" | "audio" | "signal"),
                "output" => matches!(
                    port.frame_type.as_str(),
                    "text" | "audio" | "event" | "signal"
                ),
                _ => false,
            })
        && python_available(graph)
}

struct PythonDevFactory {
    executable: PathBuf,
    source: PathBuf,
    entrypoint: String,
    default_output: Option<String>,
    connection: Option<ConnectionManifest>,
    connections: ConnectionStore,
}

impl NodeFactory for PythonDevFactory {
    fn create(
        &self,
        node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        PythonDevNode::spawn(
            self,
            self.default_output.clone(),
            config,
            &self.connections,
            node_id.clone(),
        )
        .map(|node| Box::new(node) as Box<dyn Node>)
        .map_err(|message| NodeFactoryError::new("MUXIVA-STUDIO-PYTHON-HOST", message))
    }
}

struct PythonDevNode {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    default_output: Option<String>,
    node_id: NodeId,
}

impl PythonDevNode {
    fn spawn(
        factory: &PythonDevFactory,
        default_output: Option<String>,
        config: &ConfigMap,
        connections: &ConnectionStore,
        node_id: NodeId,
    ) -> Result<Self, String> {
        let config = serde_json::Value::Object(
            config
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().to_owned(),
                        muxiva_graph_json::value_to_json(value),
                    )
                })
                .collect(),
        );
        let mut command = Command::new(&factory.executable);
        command.args([
            "-u",
            "-c",
            PYTHON_HOST,
            factory
                .source
                .to_str()
                .ok_or("Python Node path is not UTF-8")?,
            &factory.entrypoint,
            &config.to_string(),
        ]);
        connections.apply_to_command(&mut command, factory.connection.as_ref());
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stdout is the framed host protocol; stderr is the provider's
            // human-readable diagnostic channel and must reach runtime.log.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("cannot start Python Host: {error}"))?;
        let input = BufWriter::new(
            child
                .stdin
                .take()
                .ok_or("Python Host stdin is unavailable")?,
        );
        let mut output = BufReader::new(
            child
                .stdout
                .take()
                .ok_or("Python Host stdout is unavailable")?,
        );
        let ready = read_host_response(&mut output)?;
        if ready.get("ready") != Some(&serde_json::Value::Bool(true)) {
            return Err("Python Host did not complete package import".into());
        }
        Ok(Self {
            child,
            input,
            output,
            default_output,
            node_id,
        })
    }

    fn call(&mut self, command: serde_json::Value) -> Result<serde_json::Value, MuxivaError> {
        writeln!(self.input, "{command}")
            .and_then(|_| self.input.flush())
            .map_err(|error| python_error(format!("cannot send to Python Host: {error}")))?;
        let response = read_host_response(&mut self.output).map_err(python_error)?;
        if response.get("ok") == Some(&serde_json::Value::Bool(true)) {
            Ok(response)
        } else {
            Err(python_error(
                response
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Python Node failed")
                    .to_owned(),
            ))
        }
    }
}

impl Node for PythonDevNode {
    fn on_prepare(&mut self, _context: &mut NodeContext) -> muxiva_types::Result<()> {
        self.call(serde_json::json!({"op":"prepare", "node_id":self.node_id.as_str()}))
            .map(|_| ())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        let wire = input.as_ref().map(frame_to_wire).transpose()?;
        let command = serde_json::json!({
            "op":"process",
            "frame":wire,
            "input_port":context.input_port().map(PortName::as_str),
            "default_output":self.default_output,
            "node_id":context.node_id().as_str(),
        });
        writeln!(self.input, "{command}")
            .and_then(|_| self.input.flush())
            .map_err(|error| python_error(format!("cannot send to Python Host: {error}")))?;
        let response = loop {
            let response = read_host_response(&mut self.output).map_err(python_error)?;
            if response.get("kind").and_then(serde_json::Value::as_str) == Some("emission") {
                emit_python_frame(&response, input.as_ref(), context)?;
                continue;
            }
            if response.get("kind").and_then(serde_json::Value::as_str) == Some("event") {
                publish_python_event(&response, input.as_ref(), context)?;
                continue;
            }
            if response.get("kind").and_then(serde_json::Value::as_str) == Some("signal") {
                emit_python_signal(&response, input.as_ref(), context)?;
                continue;
            }
            if response.get("ok") != Some(&serde_json::Value::Bool(true)) {
                return Err(python_error(
                    response
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Python Node failed")
                        .to_owned(),
                ));
            }
            break response;
        };
        // Explicit ctx operations are streamed above. Arrays remain for Host
        // protocol compatibility with older project Nodes.
        for emission in response
            .get("emissions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            emit_python_frame(emission, input.as_ref(), context)?;
        }
        for event in response
            .get("events")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            publish_python_event(event, input.as_ref(), context)?;
        }
        for signal in response
            .get("signals")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            emit_python_signal(signal, input.as_ref(), context)?;
        }
        Ok(())
    }

    fn on_finish(&mut self, _context: &mut NodeContext) -> muxiva_types::Result<()> {
        self.call(serde_json::json!({"op":"finish", "node_id":self.node_id.as_str()}))
            .map(|_| ())
    }

    fn on_signal(
        &mut self,
        signal: muxiva_types::SignalFrame,
        context: &mut NodeContext,
    ) -> muxiva_types::Result<()> {
        self.call(serde_json::json!({
            "op":"signal",
            "node_id":self.node_id.as_str(),
            "input_port":context.input_port().map(PortName::as_str),
            "signal":{
                "kind":"signal",
                "name":signal.data().name().as_str(),
                "source":signal.data().source().as_str(),
                "schema_version":signal.data().schema_version().get(),
                "sequence":signal.header().sequence_id().get(),
            }
        }))
        .map(|_| ())
    }

    fn on_abort(&mut self, reason: &muxiva_core::AbortReason, _context: &mut NodeContext) {
        let _ = self.call(serde_json::json!({"op":"abort", "reason":reason.root().message(), "node_id":self.node_id.as_str()}));
    }
}

fn emit_python_frame(
    emission: &serde_json::Value,
    parent: Option<&Frame>,
    context: &mut NodeContext,
) -> muxiva_types::Result<()> {
    let port = emission
        .get("port")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| python_error("Python emission is missing its Port"))?;
    let frame = wire_to_frame(
        emission
            .get("frame")
            .ok_or_else(|| python_error("Python emission is missing its Frame"))?,
        parent,
        context.node_id(),
    )?;
    context.emit(
        PortName::new(port).map_err(|error| python_error(error.to_string()))?,
        frame,
    )?;
    Ok(())
}

fn publish_python_event(
    event: &serde_json::Value,
    parent: Option<&Frame>,
    context: &mut NodeContext,
) -> muxiva_types::Result<()> {
    let topic = event
        .get("topic")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| python_error("Python EventBus publication is missing its topic"))?;
    let derived = control_frame(
        parent,
        context.node_id(),
        FramePayload::Event(EventData::new(
            NamespacedName::new(topic).map_err(|error| python_error(error.to_string()))?,
            SchemaVersion::new(1).map_err(|error| python_error(error.to_string()))?,
            context.node_id().clone(),
            muxiva_graph_json::value_from_json(
                event.get("payload").unwrap_or(&serde_json::Value::Null),
            )
            .map_err(python_error)?,
        )),
    )?;
    context.publish_event(derived.as_event().expect("event payload").clone())?;
    Ok(())
}

fn emit_python_signal(
    signal: &serde_json::Value,
    parent: Option<&Frame>,
    context: &mut NodeContext,
) -> muxiva_types::Result<()> {
    let name = signal
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| python_error("Python Signal emission is missing its name"))?;
    let derived = control_frame(
        parent,
        context.node_id(),
        FramePayload::Signal(SignalData::new(
            NamespacedName::new(name).map_err(|error| python_error(error.to_string()))?,
            SchemaVersion::new(1).map_err(|error| python_error(error.to_string()))?,
            context.node_id().clone(),
            muxiva_graph_json::value_from_json(
                signal.get("payload").unwrap_or(&serde_json::Value::Null),
            )
            .map_err(python_error)?,
        )),
    )?;
    context.emit_signal(derived.as_signal().expect("signal payload").clone())?;
    Ok(())
}

impl Drop for PythonDevNode {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "{}", serde_json::json!({"op":"close"}));
        let _ = self.input.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_host_response(output: &mut BufReader<ChildStdout>) -> Result<serde_json::Value, String> {
    let mut line = String::new();
    output
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    if line.is_empty() {
        return Err("Python Host exited without a response".into());
    }
    if line.len() > MAX_HOST_RESPONSE_BYTES {
        return Err("Python Host response exceeds 1 MiB".into());
    }
    serde_json::from_str(&line).map_err(|error| format!("invalid Python Host response: {error}"))
}

fn frame_to_wire(frame: &Frame) -> muxiva_types::Result<serde_json::Value> {
    if let Some(text) = frame.as_text() {
        return Ok(serde_json::json!({
            "kind":"text",
            "text":text.data().as_str(),
            "sequence":frame.header().sequence_id().get(),
        }));
    }
    if let Some(audio) = frame.as_audio() {
        let data = audio.data();
        if data.sample_format() != PcmSampleFormat::I16Le
            || data.layout() != AudioLayout::Interleaved
        {
            return Err(python_error(
                "Studio Python Host supports interleaved PCM s16le audio",
            ));
        }
        let pcm_hex = data
            .buffer()
            .as_slice()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        return Ok(serde_json::json!({
            "kind":"audio",
            "pcm_hex":pcm_hex,
            "sample_rate_hz":data.sample_rate_hz(),
            "channels":data.channels(),
            "sequence":frame.header().sequence_id().get(),
        }));
    }
    Err(python_error(
        "Studio Python Host received an unsupported Frame",
    ))
}

fn control_frame(
    parent: Option<&Frame>,
    node_id: &NodeId,
    payload: FramePayload,
) -> muxiva_types::Result<Frame> {
    let parent = parent.ok_or_else(|| {
        python_error("source control actions require an input Frame in the Studio development Host")
    })?;
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    parent.derive(
        FrameDerivation::new(
            FrameId::new(format!("studio-python-control-{serial}")).expect("bounded frame ID"),
            parent.header().timestamp(),
            parent.header().sequence_id(),
            TransformOrigin::new(Some(node_id.clone()), None)?,
            "studio-python-control",
        )?
        .with_payload(payload),
    )
}

fn wire_to_frame(
    wire: &serde_json::Value,
    parent: Option<&Frame>,
    node_id: &NodeId,
) -> muxiva_types::Result<Frame> {
    let kind = wire
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| python_error("Python Frame is missing its kind"))?;
    let payload = match kind {
        "text" => FramePayload::Text(TextData::new(
            wire.get("text")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| python_error("Python text Frame is missing text"))?,
        )),
        "audio" => {
            let hex = wire
                .get("pcm_hex")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| python_error("Python audio Frame is missing PCM"))?;
            if hex.len() % 2 != 0 || hex.len() > 2 * 4 * 1024 * 1024 {
                return Err(python_error("Python audio PCM has an invalid size"));
            }
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let text = std::str::from_utf8(pair)
                        .map_err(|_| python_error("Python audio PCM is not hexadecimal"))?;
                    u8::from_str_radix(text, 16)
                        .map_err(|_| python_error("Python audio PCM is not hexadecimal"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let sample_rate_hz = wire
                .get("sample_rate_hz")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| python_error("Python audio Frame has an invalid sample rate"))?;
            let channels = wire
                .get("channels")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u16::try_from(value).ok())
                .ok_or_else(|| python_error("Python audio Frame has invalid channels"))?;
            let samples = bytes
                .len()
                .checked_div(2 * usize::from(channels))
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| python_error("Python audio Frame has invalid PCM length"))?;
            FramePayload::Audio(AudioData::new(
                FrameBuffer::from_vec(bytes),
                sample_rate_hz,
                channels,
                PcmSampleFormat::I16Le,
                AudioLayout::Interleaved,
                samples,
            )?)
        }
        "event" => FramePayload::Event(EventData::new(
            NamespacedName::new(
                wire.get("topic")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| python_error("Python Event Frame is missing its topic"))?,
            )?,
            SchemaVersion::new(
                wire.get("schema_version")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(1),
            )?,
            NodeId::new(
                wire.get("source")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(node_id.as_str()),
            )
            .map_err(|_| python_error("Python Event Frame has an invalid source"))?,
            muxiva_types::Value::String(
                wire.get("payload")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .into(),
            ),
        )),
        _ => return Err(python_error("Python Host emitted an unsupported Frame")),
    };
    let serial = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
    if let Some(parent) = parent {
        return parent.derive(
            FrameDerivation::new(
                FrameId::new(format!("studio-python-{serial}")).expect("bounded Studio Frame ID"),
                parent.header().timestamp(),
                parent.header().sequence_id(),
                TransformOrigin::new(Some(node_id.clone()), None)?,
                "studio_python_node",
            )?
            .with_payload(payload),
        );
    }
    Frame::new(
        FrameHeader::new(
            FrameId::new(format!("studio-python-{serial}")).expect("bounded Studio Frame ID"),
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("muxiva.studio.python").expect("valid Studio clock"),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(0),
            StreamId::new(format!("studio-python-stream-{serial}"))
                .expect("bounded Studio stream ID"),
            TraceId::new(format!("studio-python-trace-{serial}")).expect("bounded Studio trace ID"),
            payload.frame_type(),
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )?,
        payload,
    )
}

fn python_error(message: impl Into<Box<str>>) -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Internal,
        "MUXIVA-STUDIO-PYTHON-HOST",
        message,
    )
}

fn atomic_write(path: &Path, payload: &[u8]) -> io::Result<()> {
    let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("studio-{sequence}.tmp"));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(payload)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{list, register_project_nodes_with_connections, save, ConnectionStore, SaveError};
    use muxiva_core::{start_registered_runtime, EdgePolicies, RuntimeOptions};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn graph() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "muxiva-node-library-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let graph = directory.join("agent.json");
        fs::write(&graph, "{}").unwrap();
        graph
    }

    #[test]
    fn package_is_saved_outside_graph_json_and_round_trips() {
        let graph = graph();
        let input = r#"{"format":"muxiva.node/v1","package_id":"hello_python","display_name":"Hello Python","node_type":"example.hello","language":"python","factory_version":"1.0.0","kind":"transform","entrypoint":"node:HelloNode","ports":[{"name":"text_in","direction":"input","frame_type":"text"},{"name":"text_out","direction":"output","frame_type":"text"}],"config_schema":{"type":"object"},"code":"class HelloNode:\n    pass\n","runtime_available":false}"#;
        save(&graph, input).unwrap();
        let packages = list(&graph).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].manifest.node_type, "example.hello");
        assert!(graph
            .parent()
            .unwrap()
            .join(".muxiva/nodes/hello_python/node.py")
            .exists());
        fs::remove_dir_all(graph.parent().unwrap()).unwrap();
    }

    #[test]
    fn configured_provider_roots_are_discovered_as_read_only_packages() {
        let graph = graph();
        let project = graph.parent().unwrap();
        let provider = project.join("providers/algorithm/example");
        let package = provider.join("python/nodes/hello_provider");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            provider.join("muxiva.provider.json"),
            r#"{"format":"muxiva.provider/v1","provider_id":"example","display_name":"Example AI","category":"algorithm","summary":"Test algorithm provider","vendor":"Example","homepage":"https://example.test","documentation":"https://example.test/docs","license":"test-only","sdk":{"name":"Example API","version":"1"},"connections":[{"id":"example","display_name":"Example","description":"Test credentials","fields":[{"name":"api_key","label":"API Key","environment":"EXAMPLE_API_KEY","secret":true,"required":true,"default":""}]}]}"#,
        )
        .unwrap();
        fs::write(
            package.join("muxiva.node.json"),
            r#"{"format":"muxiva.node/v1","package_id":"hello_provider","display_name":"Hello Provider","node_type":"provider.example.hello","language":"python","factory_version":"1.0.0","kind":"transform","entrypoint":"node:HelloNode","category":"algorithm","capability":"language.test","summary":"Test Node","provider_id":"example","connection_id":"example","ports":[],"config_schema":{"type":"object"}}"#,
        )
        .unwrap();
        fs::write(package.join("node.py"), "class HelloNode:\n    pass\n").unwrap();
        fs::create_dir_all(project.join(".muxiva")).unwrap();
        fs::write(
            project.join(".muxiva/providers.json"),
            r#"{"format":"muxiva.providers/v1","roots":["../providers"]}"#,
        )
        .unwrap();

        let packages = list(&graph).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].origin, "provider");
        assert!(!packages[0].editable);
        assert_eq!(packages[0].manifest.category, "algorithm");
        assert_eq!(packages[0].manifest.capability, "language.test");
        assert_eq!(
            packages[0].provider_manifest.as_ref().unwrap().provider_id,
            "example"
        );
        assert_eq!(
            packages[0].resolved_connection.as_ref().unwrap().id,
            "example"
        );
        assert_eq!(
            packages[0].source_directory,
            package.canonicalize().unwrap()
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn traversal_and_unknown_languages_are_rejected() {
        let graph = graph();
        let input = r#"{"format":"muxiva.node/v1","package_id":"../escape","display_name":"Escape","node_type":"example.escape","language":"ruby","factory_version":"1.0.0","kind":"source","entrypoint":"x","ports":[],"config_schema":{},"code":"x","runtime_available":false}"#;
        assert!(matches!(save(&graph, input), Err(SaveError::Invalid(_))));
        fs::remove_dir_all(graph.parent().unwrap()).unwrap();
    }

    #[test]
    fn saved_python_node_registers_and_executes_in_the_real_runtime() {
        let graph_path = graph();
        let package = r#"{"format":"muxiva.node/v1","package_id":"uppercase_python","display_name":"Uppercase Python","node_type":"example.studio.uppercase","language":"python","factory_version":"1.0.0","kind":"transform","entrypoint":"node:MyNode","ports":[{"name":"text_in","direction":"input","frame_type":"text"},{"name":"text_out","direction":"output","frame_type":"text"}],"config_schema":{"type":"object","properties":{},"additionalProperties":false},"code":"import muxiva\nclass MyNode:\n    def on_process(self, frame, ctx):\n        ctx.emit(\"text_out\", muxiva.TextFrame(frame.text.upper(), sequence=frame.sequence))\n        ctx.publish_event(\"example.text.uppercased\", {\"sequence\": frame.sequence})\n","runtime_available":false}"#;
        save(&graph_path, package).unwrap();
        let mut registry = muxiva_graph_json::builtin_registry();
        let connections = ConnectionStore::load(&graph_path).unwrap();
        register_project_nodes_with_connections(&graph_path, &mut registry, connections).unwrap();
        let document = muxiva_graph_json::parse(r#"{"version":"muxiva.graph/v1","graph_id":"studio-python","nodes":[{"id":"source","node_type":"builtin.text_source","language":"rust","factory_version":"1.0.0","node_config":{"text":"hello"}},{"id":"python","node_type":"example.studio.uppercase","language":"python","factory_version":"1.0.0","node_config":{}},{"id":"sink","node_type":"builtin.text_sink","language":"rust","factory_version":"1.0.0","node_config":{}}],"edges":[{"id":"source-python","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"python","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}},{"id":"python-sink","from":{"node_id":"python","port":"text_out"},"to":{"node_id":"sink","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}}]}"#).unwrap();
        let graph = muxiva_graph_json::compile_with_registry(&document, &registry).unwrap();
        let runtime = start_registered_runtime(
            graph,
            &registry,
            EdgePolicies::new(),
            RuntimeOptions::default(),
        )
        .unwrap();
        assert_eq!(
            runtime.wait(Duration::from_secs(5)).unwrap().worker_total(),
            3
        );
        fs::remove_dir_all(graph_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn compiled_project_cpp_node_packs_load_through_the_real_abi() {
        let Ok(graph_path) = std::env::var("MUXIVA_VOICE_FIXTURE_GRAPH") else {
            return;
        };
        let graph_path = PathBuf::from(graph_path);
        let packages = list(&graph_path).unwrap();
        let cpp_packages = packages
            .iter()
            .filter(|package| package.manifest.language == "cpp")
            .collect::<Vec<_>>();
        assert!(!cpp_packages.is_empty());
        assert!(cpp_packages.iter().all(|package| package.runtime_available));
        let mut registry = muxiva_graph_json::builtin_registry();
        let connections = ConnectionStore::load(&graph_path).unwrap();
        register_project_nodes_with_connections(&graph_path, &mut registry, connections).unwrap();
        for package in cpp_packages {
            assert!(registry
                .resolve(
                    &muxiva_core::NodeTypeName::new(package.manifest.node_type.clone()).unwrap(),
                    muxiva_core::NodeLanguage::Cpp,
                    &muxiva_core::NodeFactoryVersion::new(package.manifest.factory_version.clone())
                        .unwrap(),
                )
                .is_ok());
        }
    }
}
