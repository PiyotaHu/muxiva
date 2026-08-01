use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use voxa_core::{
    ConfigMap, ConfigSchema, LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeFactory,
    NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration, NodeRegistry,
    NodeTypeName, PortDescriptor, PortDirection, PortName,
};
use voxa_types::{
    AudioData, AudioLayout, ClockDomain, ClockDomainId, ClockKind, ErrorCategory, EventData,
    Extensions, Frame, FrameBuffer, FrameDerivation, FrameHeader, FrameId, FramePayload, FrameType,
    Lineage, Metadata, NamespacedName, NodeId, PcmSampleFormat, SchemaVersion, SequenceId,
    SignalData, StreamId, TextData, Timestamp, TraceId, TransformOrigin, VoxaError,
};

const FORMAT: &str = "voxa.node/v1";
const MAX_CODE_BYTES: usize = 512 * 1024;
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

shim = types.ModuleType("voxa")
shim.TextFrame = TextFrame
shim.AudioFrame = AudioFrame

class NodeContext:
    def __init__(self, node_id, input_port, config):
        self.node_id, self.input_port, self.config = node_id, input_port, config
        self.emissions, self.signals, self.events = [], [], []
    def emit(self, port, frame): self.emissions.append({"port":port, "frame":encode_frame(frame)})
    def emit_signal(self, name, payload=None): self.signals.append({"name":name, "payload":payload})
    def publish_event(self, topic, payload=None): self.events.append({"topic":topic, "payload":payload})
    def __str__(self): return self.input_port or ""
    def __eq__(self, other): return self.input_port == other

shim.NodeContext = NodeContext
sys.modules["voxa"] = shim

source, entrypoint, config_json = sys.argv[1:4]
spec = importlib.util.spec_from_file_location("voxa_project_node", source)
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
    raise ValueError("Studio Python Host received an unsupported Frame")

def encode_frame(value):
    if isinstance(value, TextFrame): return {"kind":"text", "text":value.text, "sequence":value.sequence}
    if isinstance(value, AudioFrame): return {"kind":"audio", "pcm_hex":value.data.hex(), "sample_rate_hz":value.sample_rate_hz, "channels":value.channels, "sequence":value.sequence}
    if isinstance(value, dict) and value.get("kind") == "text": return value
    raise ValueError("Python Node emitted an unsupported Frame")

print(json.dumps({"ready": True}), flush=True)
for line in sys.stdin:
    try:
        command = json.loads(line)
        op = command["op"]
        if op == "process":
            frame = decode_frame(command.get("frame"))
            ctx = NodeContext(command["node_id"], command.get("input_port"), config)
            result = invoke("on_process", frame, ctx)
            emissions = list(ctx.emissions)
            if result is not None:
                values = result if isinstance(result, dict) else {command["default_output"]: result}
                for port, frames in values.items():
                    if not isinstance(frames, list): frames = [frames]
                    emissions.extend({"port":port, "frame":encode_frame(item)} for item in frames)
            response = {"ok": True, "emissions": emissions, "signals":ctx.signals, "events":ctx.events}
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
    pub ports: Vec<NodePortManifest>,
    pub config_schema: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<ConnectionManifest>,
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
    #[serde(default)]
    pub default: String,
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
}

impl ConnectionStore {
    pub fn load(graph: &Path) -> Result<Self, String> {
        let mut manifests = BTreeMap::<String, ConnectionManifest>::new();
        for package in list(graph).map_err(|error| error.to_string())? {
            let Some(connection) = package.manifest.connection else {
                continue;
            };
            match manifests.get(&connection.id) {
                Some(existing) if existing != &connection => {
                    return Err(format!(
                        "Node packages declare conflicting connection `{}`",
                        connection.id
                    ))
                }
                Some(_) => {}
                None => {
                    manifests.insert(connection.id.clone(), connection);
                }
            }
        }
        let manifests = manifests.into_values().collect::<Vec<_>>();
        let mut values = BTreeMap::new();
        for connection in &manifests {
            for field in &connection.fields {
                let value = std::env::var(&field.environment)
                    .ok()
                    .unwrap_or_else(|| field.default.clone());
                if !value.is_empty() {
                    values.insert(
                        (connection.id.clone(), field.name.clone()),
                        SecretBytes(value.into_bytes()),
                    );
                }
            }
        }
        Ok(Self {
            manifests: Arc::new(manifests),
            values: Arc::new(Mutex::new(values)),
        })
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
            "storage": "process-memory",
        })
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
                if field.secret && value.is_empty() {
                    continue;
                }
                values
                    .entry((connection_id.clone(), name.clone()))
                    .or_insert_with(|| SecretBytes(Vec::new()))
                    .replace(value.trim());
            }
        }
        Ok(())
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodePackage {
    #[serde(flatten)]
    pub manifest: NodePackageManifest,
    pub code: String,
    pub runtime_available: bool,
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
    let root = library_root(graph);
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut directories = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    directories.sort();
    directories
        .into_iter()
        .map(read_package)
        .collect::<io::Result<Vec<_>>>()
}

pub fn register_project_nodes_with_connections(
    graph: &Path,
    registry: &mut NodeRegistry,
    connections: ConnectionStore,
) -> Result<(), String> {
    for package in list(graph).map_err(|error| error.to_string())? {
        if !python_host_supported(&package) {
            continue;
        }
        let registration = python_registration(graph, &package, connections.clone())?;
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
    let directory = library_root(graph).join(&package.manifest.package_id);
    fs::create_dir_all(&directory)?;
    atomic_write(
        &directory.join("voxa.node.json"),
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

fn library_root(graph: &Path) -> PathBuf {
    graph
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".voxa")
        .join("nodes")
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

fn read_package(directory: PathBuf) -> io::Result<NodePackage> {
    let manifest: NodePackageManifest =
        serde_json::from_str(&fs::read_to_string(directory.join("voxa.node.json"))?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let code = fs::read_to_string(directory.join(source_filename(&manifest.language)))?;
    let runtime_available = python_host_supported_manifest(&manifest);
    Ok(NodePackage {
        manifest,
        code,
        runtime_available,
    })
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
        ConfigSchema::new(voxa_graph_json::value_from_json(&manifest.config_schema)?),
        LifecycleCapabilities::new(true, true, true, true),
    );
    let source = library_root(graph)
        .join(&manifest.package_id)
        .join(source_filename("python"));
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
            source,
            entrypoint: manifest.entrypoint.clone(),
            default_output,
            connection: manifest.connection.clone(),
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

fn python_executable() -> String {
    std::env::var("VOXA_PYTHON").unwrap_or_else(|_| "python3".into())
}

fn python_available() -> bool {
    Command::new(python_executable())
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn python_host_supported(package: &NodePackage) -> bool {
    python_host_supported_manifest(&package.manifest)
}

fn python_host_supported_manifest(manifest: &NodePackageManifest) -> bool {
    manifest.language == "python"
        && manifest
            .ports
            .iter()
            .all(|port| matches!(port.frame_type.as_str(), "text" | "audio"))
        && python_available()
}

struct PythonDevFactory {
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
            &self.source,
            &self.entrypoint,
            self.default_output.clone(),
            config,
            &self.connections,
            self.connection.as_ref(),
            node_id.clone(),
        )
        .map(|node| Box::new(node) as Box<dyn Node>)
        .map_err(|message| NodeFactoryError::new("VOXA-STUDIO-PYTHON-HOST", message))
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
        source: &Path,
        entrypoint: &str,
        default_output: Option<String>,
        config: &ConfigMap,
        connections: &ConnectionStore,
        connection: Option<&ConnectionManifest>,
        node_id: NodeId,
    ) -> Result<Self, String> {
        let config = serde_json::Value::Object(
            config
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().to_owned(),
                        voxa_graph_json::value_to_json(value),
                    )
                })
                .collect(),
        );
        let mut command = Command::new(python_executable());
        command.args([
            "-u",
            "-c",
            PYTHON_HOST,
            source.to_str().ok_or("Python Node path is not UTF-8")?,
            entrypoint,
            &config.to_string(),
        ]);
        connections.apply_to_command(&mut command, connection);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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

    fn call(&mut self, command: serde_json::Value) -> Result<serde_json::Value, VoxaError> {
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
    fn on_prepare(&mut self, _context: &mut NodeContext) -> voxa_types::Result<()> {
        self.call(serde_json::json!({"op":"prepare", "node_id":self.node_id.as_str()}))
            .map(|_| ())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let wire = input.as_ref().map(frame_to_wire).transpose()?;
        let response = self.call(serde_json::json!({
            "op":"process",
            "frame":wire,
            "input_port":context.input_port().map(PortName::as_str),
            "default_output":self.default_output,
            "node_id":context.node_id().as_str(),
        }))?;
        for emission in response
            .get("emissions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let port = emission
                .get("port")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| python_error("Python emission is missing its Port"))?;
            let frame = wire_to_frame(
                emission
                    .get("frame")
                    .ok_or_else(|| python_error("Python emission is missing its Frame"))?,
                input.as_ref(),
                context.node_id(),
            )?;
            context.emit(
                PortName::new(port).map_err(|error| python_error(error.to_string()))?,
                frame,
            )?;
        }
        for event in response
            .get("events")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let topic = event
                .get("topic")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| python_error("Python EventBus publication is missing its topic"))?;
            let derived = control_frame(
                input.as_ref(),
                context.node_id(),
                FramePayload::Event(EventData::new(
                    NamespacedName::new(topic).map_err(|error| python_error(error.to_string()))?,
                    SchemaVersion::new(1).map_err(|error| python_error(error.to_string()))?,
                    context.node_id().clone(),
                    voxa_graph_json::value_from_json(
                        event.get("payload").unwrap_or(&serde_json::Value::Null),
                    )
                    .map_err(python_error)?,
                )),
            )?;
            context.publish_event(derived.as_event().expect("event payload").clone())?;
        }
        for signal in response
            .get("signals")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = signal
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| python_error("Python Signal emission is missing its name"))?;
            let derived = control_frame(
                input.as_ref(),
                context.node_id(),
                FramePayload::Signal(SignalData::new(
                    NamespacedName::new(name).map_err(|error| python_error(error.to_string()))?,
                    SchemaVersion::new(1).map_err(|error| python_error(error.to_string()))?,
                    context.node_id().clone(),
                    voxa_graph_json::value_from_json(
                        signal.get("payload").unwrap_or(&serde_json::Value::Null),
                    )
                    .map_err(python_error)?,
                )),
            )?;
            context.emit_signal(derived.as_signal().expect("signal payload").clone())?;
        }
        Ok(())
    }

    fn on_finish(&mut self, _context: &mut NodeContext) -> voxa_types::Result<()> {
        self.call(serde_json::json!({"op":"finish", "node_id":self.node_id.as_str()}))
            .map(|_| ())
    }

    fn on_abort(&mut self, reason: &voxa_core::AbortReason, _context: &mut NodeContext) {
        let _ = self.call(serde_json::json!({"op":"abort", "reason":reason.root().message(), "node_id":self.node_id.as_str()}));
    }
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

fn frame_to_wire(frame: &Frame) -> voxa_types::Result<serde_json::Value> {
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
) -> voxa_types::Result<Frame> {
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
) -> voxa_types::Result<Frame> {
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
                ClockDomainId::new("voxa.studio.python").expect("valid Studio clock"),
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

fn python_error(message: impl Into<Box<str>>) -> VoxaError {
    VoxaError::new(ErrorCategory::Internal, "VOXA-STUDIO-PYTHON-HOST", message)
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
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };
    use voxa_core::{start_registered_runtime, EdgePolicies, RuntimeOptions};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn graph() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "voxa-node-library-{}-{}",
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
        let input = r#"{"format":"voxa.node/v1","package_id":"hello_python","display_name":"Hello Python","node_type":"example.hello","language":"python","factory_version":"1.0.0","kind":"transform","entrypoint":"node:HelloNode","ports":[{"name":"text_in","direction":"input","frame_type":"text"},{"name":"text_out","direction":"output","frame_type":"text"}],"config_schema":{"type":"object"},"code":"class HelloNode:\n    pass\n","runtime_available":false}"#;
        save(&graph, input).unwrap();
        let packages = list(&graph).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].manifest.node_type, "example.hello");
        assert!(graph
            .parent()
            .unwrap()
            .join(".voxa/nodes/hello_python/node.py")
            .exists());
        fs::remove_dir_all(graph.parent().unwrap()).unwrap();
    }

    #[test]
    fn traversal_and_unknown_languages_are_rejected() {
        let graph = graph();
        let input = r#"{"format":"voxa.node/v1","package_id":"../escape","display_name":"Escape","node_type":"example.escape","language":"ruby","factory_version":"1.0.0","kind":"source","entrypoint":"x","ports":[],"config_schema":{},"code":"x","runtime_available":false}"#;
        assert!(matches!(save(&graph, input), Err(SaveError::Invalid(_))));
        fs::remove_dir_all(graph.parent().unwrap()).unwrap();
    }

    #[test]
    fn saved_python_node_registers_and_executes_in_the_real_runtime() {
        let graph_path = graph();
        let package = r#"{"format":"voxa.node/v1","package_id":"uppercase_python","display_name":"Uppercase Python","node_type":"example.studio.uppercase","language":"python","factory_version":"1.0.0","kind":"transform","entrypoint":"node:MyNode","ports":[{"name":"text_in","direction":"input","frame_type":"text"},{"name":"text_out","direction":"output","frame_type":"text"}],"config_schema":{"type":"object","properties":{},"additionalProperties":false},"code":"import voxa\nclass MyNode:\n    def on_process(self, frame, ctx):\n        ctx.emit(\"text_out\", voxa.TextFrame(frame.text.upper(), sequence=frame.sequence))\n        ctx.publish_event(\"example.text.uppercased\", {\"sequence\": frame.sequence})\n","runtime_available":false}"#;
        save(&graph_path, package).unwrap();
        let mut registry = voxa_graph_json::builtin_registry();
        let connections = ConnectionStore::load(&graph_path).unwrap();
        register_project_nodes_with_connections(&graph_path, &mut registry, connections).unwrap();
        let document = voxa_graph_json::parse(r#"{"version":"voxa.graph/v1","graph_id":"studio-python","nodes":[{"id":"source","node_type":"builtin.text_source","language":"rust","factory_version":"1.0.0","node_config":{"text":"hello"}},{"id":"python","node_type":"example.studio.uppercase","language":"python","factory_version":"1.0.0","node_config":{}},{"id":"sink","node_type":"builtin.text_sink","language":"rust","factory_version":"1.0.0","node_config":{}}],"edges":[{"id":"source-python","from":{"node_id":"source","port":"text_out"},"to":{"node_id":"python","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}},{"id":"python-sink","from":{"node_id":"python","port":"text_out"},"to":{"node_id":"sink","port":"text_in"},"frame_type":"text","queue_policy":{"capacity":8,"overflow":"block"}}]}"#).unwrap();
        let graph = voxa_graph_json::compile_with_registry(&document, &registry).unwrap();
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
}
