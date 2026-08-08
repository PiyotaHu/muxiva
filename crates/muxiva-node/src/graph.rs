//! Registry-driven Graph v1 execution hosted by a JavaScript Worker.

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

use muxiva_core::{
    start_registered_runtime, ConfigMap, ConfigSchema, EdgePolicies, ForeignNodeCallOutput,
    ForeignNodeConstructor, ForeignNodeFactoryAdapter, ForeignNodeInstance, LifecycleCapabilities,
    NodeDescriptor, NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration,
    NodeTypeName, PortDescriptor, PortDirection, PortName, RuntimeOptions, RuntimeWaitError,
};
use muxiva_types::{
    ErrorCategory, EventData, Frame, FrameDerivation, FrameId, FramePayload, FrameType,
    MuxivaError, NamespacedName, NodeId, SchemaVersion, SignalData, SignalFrame, TransformOrigin,
};
use napi::{
    bindgen_prelude::{AsyncTask, Function},
    threadsafe_function::{ThreadsafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode},
    Env, Error, Status, Task,
};
use napi_derive::napi;
use serde::Deserialize;

use crate::frame::{frame_from_wire, frame_to_wire};

const MAX_TIMEOUT_MS: u32 = 60 * 60 * 1_000;
const CALLBACK_QUEUE_CAPACITY: usize = 1024;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(10);
static NEXT_ACTION_FRAME: AtomicU64 = AtomicU64::new(1);

#[napi(object)]
pub struct GraphFactorySpec {
    pub node_type: String,
    pub version: String,
    pub input_port: String,
    pub output_port: String,
    pub kind: Option<String>,
    pub ports_json: Option<String>,
    pub config_schema_json: Option<String>,
}

#[derive(Clone)]
struct FactorySpec {
    node_type: NodeTypeName,
    version: NodeFactoryVersion,
    kind: NodeKind,
    ports: Vec<FactoryPort>,
    config_schema: ConfigSchema,
}

#[derive(Clone)]
struct FactoryPort {
    name: PortName,
    direction: PortDirection,
    frame_type: FrameType,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FactoryPortDocument {
    name: String,
    direction: String,
    frame_type: String,
}

#[derive(Clone)]
struct GraphCommand {
    factory_key: String,
    node_type: String,
    node_id: String,
    kind: String,
    payload_json: Option<String>,
    input_port: Option<String>,
    config_json: String,
}

#[napi(object)]
pub struct JsGraphCommand {
    pub factory_key: String,
    pub node_type: String,
    pub node_id: String,
    pub kind: String,
    pub payload_json: Option<String>,
    pub input_port: Option<String>,
    pub config_json: String,
}

type GraphCallback =
    Arc<ThreadsafeFunction<GraphCommand, String, JsGraphCommand, Status, false, false, 1024>>;

struct TypeScriptNodeConstructor {
    callback: GraphCallback,
    factory_key: String,
    node_type: String,
    output_ports: Vec<PortName>,
}

impl ForeignNodeConstructor for TypeScriptNodeConstructor {
    fn create(
        &self,
        node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError> {
        Ok(Box::new(TypeScriptInstance {
            callback: self.callback.clone(),
            factory_key: self.factory_key.clone(),
            node_type: self.node_type.clone(),
            node_id: node_id.as_str().to_owned(),
            output_ports: self.output_ports.clone(),
            config_json: muxiva_graph_json::config_map_to_json(config).to_string(),
        }))
    }
}

struct TypeScriptInstance {
    callback: GraphCallback,
    factory_key: String,
    node_type: String,
    node_id: String,
    output_ports: Vec<PortName>,
    config_json: String,
}

impl TypeScriptInstance {
    fn invoke(
        &self,
        kind: &str,
        payload_json: Option<String>,
        input_port: Option<String>,
    ) -> Result<GraphResponse, MuxivaError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let status = self.callback.call_with_return_value(
            GraphCommand {
                factory_key: self.factory_key.clone(),
                node_type: self.node_type.clone(),
                node_id: self.node_id.clone(),
                kind: kind.to_owned(),
                payload_json,
                input_port,
                config_json: self.config_json.clone(),
            },
            ThreadsafeFunctionCallMode::NonBlocking,
            move |response, _env| {
                let _ = sender.send(response);
                Ok(())
            },
        );
        if status != Status::Ok {
            return Err(graph_error(
                "MUXIVA-NODE-GRAPH-CALLBACK",
                "TypeScript callback queue is full or closing",
            ));
        }
        let encoded = receiver.recv_timeout(CALLBACK_TIMEOUT).map_err(|_| {
            graph_error(
                "MUXIVA-NODE-GRAPH-DEADLINE",
                "TypeScript lifecycle callback exceeded its deadline",
            )
        })?;
        let encoded = encoded
            .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-CALLBACK", &error.to_string()))?;
        let response: GraphResponse = serde_json::from_str(&encoded).map_err(|_| {
            graph_error(
                "MUXIVA-NODE-GRAPH-RESPONSE",
                "TypeScript callback returned an invalid response envelope",
            )
        })?;
        if response.ok {
            Ok(response)
        } else {
            let error = response.error.unwrap_or(GraphResponseError {
                code: "MUXIVA-NODE-EXCEPTION".to_owned(),
                message: "TypeScript callback failed".to_owned(),
            });
            Err(graph_error(&error.code, &error.message))
        }
    }
}

impl ForeignNodeInstance for TypeScriptInstance {
    fn on_prepare(&mut self) -> Result<ForeignNodeCallOutput, MuxivaError> {
        self.invoke("prepare", None, None)?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        input_port: Option<&PortName>,
    ) -> Result<ForeignNodeCallOutput, MuxivaError> {
        let payload = input
            .as_ref()
            .map(frame_to_wire)
            .transpose()
            .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-INPUT", &error.to_string()))?
            .map(|value| value.to_string());
        let response = self.invoke(
            "process",
            payload,
            input_port.map(|port| port.as_str().to_owned()),
        )?;
        let action_emissions = response.actions_emissions(&self.output_ports)?;
        let mut signals = Vec::new();
        let mut events = Vec::new();
        for action in response.actions {
            match action {
                GraphAction::Emit { .. } => {}
                GraphAction::Signal { name, payload } => signals.push(
                    control_action_frame(input.as_ref(), &self.node_id, &name, payload, true)?
                        .as_signal()
                        .expect("signal action")
                        .clone(),
                ),
                GraphAction::Event { topic, payload } => events.push(
                    control_action_frame(input.as_ref(), &self.node_id, &topic, payload, false)?
                        .as_event()
                        .expect("event action")
                        .clone(),
                ),
            }
        }
        let mut output = decode_emissions(response.value, &self.output_ports)?;
        output = output
            .with_additional_emissions(action_emissions)
            .with_signals(signals)
            .with_events(events);
        Ok(output)
    }

    fn on_signal(&mut self, _signal: SignalFrame) -> Result<ForeignNodeCallOutput, MuxivaError> {
        self.invoke("signal", None, None)?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_finish(&mut self) -> Result<ForeignNodeCallOutput, MuxivaError> {
        self.invoke("finish", None, None)?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_abort(&mut self, reason: &muxiva_core::AbortReason) {
        let payload = serde_json::json!({
            "code": reason.root().code(),
            "message": reason.root().message()
        })
        .to_string();
        let _ = self.invoke("abort", Some(payload), None);
    }
}

#[derive(Deserialize)]
struct GraphResponse {
    ok: bool,
    value: Option<serde_json::Value>,
    error: Option<GraphResponseError>,
    #[serde(default)]
    actions: Vec<GraphAction>,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum GraphAction {
    Emit {
        port: String,
        frame: serde_json::Value,
    },
    Signal {
        name: String,
        payload: serde_json::Value,
    },
    Event {
        topic: String,
        payload: serde_json::Value,
    },
}

impl GraphResponse {
    fn actions_emissions(
        &self,
        output_ports: &[PortName],
    ) -> Result<Vec<muxiva_core::ForeignNodeEmission>, MuxivaError> {
        self.actions
            .iter()
            .filter_map(|action| match action {
                GraphAction::Emit { port, frame } => Some((port, frame)),
                _ => None,
            })
            .map(|(name, value)| {
                let port = output_ports
                    .iter()
                    .find(|port| port.as_str() == name)
                    .ok_or_else(|| {
                        graph_error(
                            "MUXIVA-NODE-GRAPH-OUTPUT-PORT",
                            "ctx.emit used an undeclared output port",
                        )
                    })?;
                Ok(muxiva_core::ForeignNodeEmission::new(
                    port.clone(),
                    frame_from_wire(value).map_err(|error| {
                        graph_error("MUXIVA-NODE-GRAPH-OUTPUT", &error.to_string())
                    })?,
                ))
            })
            .collect()
    }
}

fn control_action_frame(
    parent: Option<&Frame>,
    node_id: &str,
    name: &str,
    payload: serde_json::Value,
    signal: bool,
) -> Result<Frame, MuxivaError> {
    let parent = parent.ok_or_else(|| {
        graph_error(
            "MUXIVA-NODE-GRAPH-CONTROL-SOURCE",
            "source control actions require an input Frame in Node V1",
        )
    })?;
    let source = NodeId::new(node_id.to_owned())
        .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-NODE-ID", &error.to_string()))?;
    let namespace = NamespacedName::new(name.to_owned())
        .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-CONTROL-NAME", &error.to_string()))?;
    let value = muxiva_graph_json::value_from_json(&payload)
        .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-CONTROL-PAYLOAD", &error))?;
    let payload = if signal {
        FramePayload::Signal(SignalData::new(
            namespace,
            SchemaVersion::new(1).unwrap(),
            source.clone(),
            value,
        ))
    } else {
        FramePayload::Event(EventData::new(
            namespace,
            SchemaVersion::new(1).unwrap(),
            source.clone(),
            value,
        ))
    };
    let serial = NEXT_ACTION_FRAME.fetch_add(1, Ordering::Relaxed);
    parent
        .derive(
            FrameDerivation::new(
                FrameId::new(format!(
                    "node-action-{}-{}-{}-{}",
                    node_id,
                    parent.header().sequence_id().get(),
                    if signal { "signal" } else { "event" },
                    serial,
                ))
                .map_err(|error| {
                    graph_error("MUXIVA-NODE-GRAPH-CONTROL-FRAME", &error.to_string())
                })?,
                parent.header().timestamp(),
                parent.header().sequence_id(),
                TransformOrigin::new(Some(source), None).map_err(|error| {
                    graph_error("MUXIVA-NODE-GRAPH-CONTROL-ORIGIN", &error.to_string())
                })?,
                "typescript-context-action",
            )
            .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-CONTROL-DERIVE", &error.to_string()))?
            .with_payload(payload),
        )
        .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-CONTROL-DERIVE", &error.to_string()))
}

#[derive(Deserialize)]
struct GraphResponseError {
    code: String,
    message: String,
}

pub struct RunRegisteredGraphTask {
    graph_json: String,
    factories: Vec<FactorySpec>,
    callback: GraphCallback,
    timeout: Duration,
}

impl Task for RunRegisteredGraphTask {
    type Output = u32;
    type JsValue = u32;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let mut registry = muxiva_graph_json::builtin_registry();
        for spec in &self.factories {
            registry
                .register(registration(spec, self.callback.clone()))
                .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?;
        }
        let document = muxiva_graph_json::parse(&self.graph_json).map_err(diagnostics)?;
        let graph =
            muxiva_graph_json::compile_with_registry(&document, &registry).map_err(diagnostics)?;
        let runtime = start_registered_runtime(
            graph,
            &registry,
            EdgePolicies::new(),
            RuntimeOptions::default(),
        )
        .map_err(|error| Error::new(Status::GenericFailure, error.to_string()))?;
        match runtime.wait(self.timeout) {
            Ok(summary) => u32::try_from(summary.worker_total())
                .map_err(|_| Error::new(Status::GenericFailure, "worker count exceeds u32")),
            Err(RuntimeWaitError::Aborted(reason)) => Err(Error::new(
                Status::GenericFailure,
                format!("{}: {}", reason.root().code(), reason.root().message()),
            )),
            Err(RuntimeWaitError::Timeout(diagnostics)) => {
                runtime.stop();
                let _ = runtime.wait(Duration::from_secs(5));
                Err(Error::new(
                    Status::GenericFailure,
                    format!(
                        "MUXIVA-NODE-GRAPH-TIMEOUT: active nodes [{}]",
                        diagnostics
                            .active_nodes()
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                ))
            }
        }
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output)
    }
}

#[napi]
#[allow(clippy::needless_pass_by_value)]
pub fn run_registered_graph(
    graph_json: String,
    factories: Vec<GraphFactorySpec>,
    callback: Function<'_, JsGraphCommand, String>,
    timeout_ms: Option<u32>,
) -> napi::Result<AsyncTask<RunRegisteredGraphTask>> {
    let timeout_ms = timeout_ms.unwrap_or(30_000);
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return Err(Error::new(
            Status::InvalidArg,
            format!("timeoutMs must be between 1 and {MAX_TIMEOUT_MS}"),
        ));
    }
    let factories = factories
        .into_iter()
        .map(parse_spec)
        .collect::<napi::Result<Vec<_>>>()?;
    let callback = Arc::new(
        callback
            .build_threadsafe_function::<GraphCommand>()
            .max_queue_size::<CALLBACK_QUEUE_CAPACITY>()
            .build_callback(|context: ThreadsafeCallContext<GraphCommand>| {
                Ok(JsGraphCommand {
                    factory_key: context.value.factory_key,
                    node_type: context.value.node_type,
                    node_id: context.value.node_id,
                    kind: context.value.kind,
                    payload_json: context.value.payload_json,
                    input_port: context.value.input_port,
                    config_json: context.value.config_json,
                })
            })?,
    );
    Ok(AsyncTask::new(RunRegisteredGraphTask {
        graph_json,
        factories,
        callback,
        timeout: Duration::from_millis(timeout_ms.into()),
    }))
}

fn parse_spec(spec: GraphFactorySpec) -> napi::Result<FactorySpec> {
    let kind = match spec.kind.as_deref().unwrap_or("transform") {
        "source" => NodeKind::Source,
        "transform" => NodeKind::Transform,
        "sink" => NodeKind::Sink,
        _ => {
            return Err(Error::new(
                Status::InvalidArg,
                "kind must be source, transform, or sink",
            ))
        }
    };
    let ports = match spec.ports_json {
        Some(encoded) => serde_json::from_str::<Vec<FactoryPortDocument>>(&encoded)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?
            .into_iter()
            .map(parse_port)
            .collect::<napi::Result<Vec<_>>>()?,
        None => vec![
            FactoryPort {
                name: PortName::new(spec.input_port)
                    .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
                direction: PortDirection::Input,
                frame_type: FrameType::Text,
            },
            FactoryPort {
                name: PortName::new(spec.output_port)
                    .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
                direction: PortDirection::Output,
                frame_type: FrameType::Text,
            },
        ],
    };
    let schema: serde_json::Value =
        serde_json::from_str(spec.config_schema_json.as_deref().unwrap_or("{}"))
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?;
    Ok(FactorySpec {
        node_type: NodeTypeName::new(spec.node_type)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
        version: NodeFactoryVersion::new(spec.version)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
        kind,
        ports,
        config_schema: ConfigSchema::new(
            muxiva_graph_json::value_from_json(&schema)
                .map_err(|error| Error::new(Status::InvalidArg, error))?,
        ),
    })
}

fn registration(spec: &FactorySpec, callback: GraphCallback) -> NodeRegistration {
    let template = NodeId::new(format!("template-{}", spec.node_type.as_str()))
        .expect("valid node type produces valid template ID");
    let descriptor = NodeDescriptor::new(
        template.clone(),
        spec.node_type.clone(),
        spec.kind,
        spec.ports
            .iter()
            .map(|port| {
                PortDescriptor::new(
                    template.clone(),
                    port.name.clone(),
                    port.direction,
                    port.frame_type,
                )
            })
            .collect::<Vec<_>>(),
        spec.config_schema.clone(),
        LifecycleCapabilities::new(true, true, true, true),
    );
    let constructor = TypeScriptNodeConstructor {
        callback,
        factory_key: format!("{}@{}", spec.node_type.as_str(), spec.version.as_str()),
        node_type: spec.node_type.as_str().to_owned(),
        output_ports: spec
            .ports
            .iter()
            .filter(|port| port.direction == PortDirection::Output)
            .map(|port| port.name.clone())
            .collect(),
    };
    NodeRegistration::new(
        NodeLanguage::TypeScript,
        descriptor,
        spec.version.clone(),
        Arc::new(ForeignNodeFactoryAdapter::new(Arc::new(constructor))),
    )
}

fn diagnostics(errors: Vec<muxiva_graph_json::GraphDiagnostic>) -> Error {
    Error::new(
        Status::InvalidArg,
        errors
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn graph_error(code: &str, message: &str) -> MuxivaError {
    MuxivaError::try_new(ErrorCategory::External, code.to_owned(), message.to_owned())
        .unwrap_or_else(|_| {
            MuxivaError::new(
                ErrorCategory::External,
                "MUXIVA-NODE-GRAPH",
                "TypeScript Graph execution failed",
            )
        })
}

fn parse_port(document: FactoryPortDocument) -> napi::Result<FactoryPort> {
    let direction = match document.direction.as_str() {
        "input" => PortDirection::Input,
        "output" => PortDirection::Output,
        _ => {
            return Err(Error::new(
                Status::InvalidArg,
                "port direction must be input or output",
            ))
        }
    };
    let frame_type = match document.frame_type.as_str() {
        "audio" => FrameType::Audio,
        "video" => FrameType::Video,
        "text" => FrameType::Text,
        "byte" => FrameType::Byte,
        _ => {
            return Err(Error::new(
                Status::InvalidArg,
                "port frameType must be audio, video, text, or byte",
            ))
        }
    };
    Ok(FactoryPort {
        name: PortName::new(document.name)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
        direction,
        frame_type,
    })
}

fn decode_emissions(
    value: Option<serde_json::Value>,
    output_ports: &[PortName],
) -> Result<ForeignNodeCallOutput, MuxivaError> {
    let Some(value) = value else {
        return Ok(ForeignNodeCallOutput::default());
    };
    let mut emissions = Vec::new();
    if output_ports.len() == 1 && value.get("kind").is_some() {
        let frame = frame_from_wire(&value)
            .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-OUTPUT", &error.to_string()))?;
        emissions.push(muxiva_core::ForeignNodeEmission::new(
            output_ports[0].clone(),
            frame,
        ));
        return Ok(ForeignNodeCallOutput::new(emissions, []));
    }
    let mapping = value.as_object().ok_or_else(|| {
        graph_error(
        "MUXIVA-NODE-GRAPH-OUTPUT",
        "a callback with zero or multiple output ports must return an object keyed by output port",
    )
    })?;
    for (name, frames) in mapping {
        let port = output_ports
            .iter()
            .find(|port| port.as_str() == name)
            .ok_or_else(|| {
                graph_error(
                    "MUXIVA-NODE-GRAPH-OUTPUT-PORT",
                    "callback emitted an undeclared output port",
                )
            })?;
        if let Some(values) = frames.as_array() {
            for value in values {
                emissions.push(muxiva_core::ForeignNodeEmission::new(
                    port.clone(),
                    frame_from_wire(value).map_err(|error| {
                        graph_error("MUXIVA-NODE-GRAPH-OUTPUT", &error.to_string())
                    })?,
                ));
            }
        } else {
            emissions.push(muxiva_core::ForeignNodeEmission::new(
                port.clone(),
                frame_from_wire(frames)
                    .map_err(|error| graph_error("MUXIVA-NODE-GRAPH-OUTPUT", &error.to_string()))?,
            ));
        }
    }
    Ok(ForeignNodeCallOutput::new(emissions, []))
}
