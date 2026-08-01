//! Registry-driven Graph v1 execution hosted by a JavaScript Worker.

use std::{
    sync::{mpsc, Arc},
    time::Duration,
};

use napi::{
    bindgen_prelude::AsyncTask,
    threadsafe_function::{
        ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
    },
    Env, Error, JsFunction, Status, Task,
};
use napi_derive::napi;
use serde::Deserialize;
use voxa_core::{
    start_registered_runtime, ConfigMap, ConfigSchema, EdgePolicies, ForeignNodeCallOutput,
    ForeignNodeFactoryAdapter, ForeignNodeInstance, ForeignNodeProvider, LifecycleCapabilities,
    NodeDescriptor, NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration,
    NodeTypeName, PortDescriptor, PortDirection, PortName, RuntimeOptions, RuntimeWaitError,
};
use voxa_types::{ErrorCategory, Frame, FrameType, NodeId, SignalFrame, VoxaError};

use crate::frame::owned_text_frame;

const MAX_TIMEOUT_MS: u32 = 60 * 60 * 1_000;
const CALLBACK_QUEUE_CAPACITY: usize = 1024;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(10);

#[napi(object)]
pub struct GraphFactorySpec {
    pub node_type: String,
    pub version: String,
    pub input_port: String,
    pub output_port: String,
}

#[derive(Clone)]
struct FactorySpec {
    node_type: NodeTypeName,
    version: NodeFactoryVersion,
    input_port: PortName,
    output_port: PortName,
}

#[derive(Clone)]
struct GraphCommand {
    factory_key: String,
    node_type: String,
    node_id: String,
    kind: String,
    payload_json: Option<String>,
}

#[napi(object)]
pub struct JsGraphCommand {
    pub factory_key: String,
    pub node_type: String,
    pub node_id: String,
    pub kind: String,
    pub payload_json: Option<String>,
}

type GraphCallback = ThreadsafeFunction<GraphCommand, ErrorStrategy::Fatal>;

struct TypeScriptProvider {
    callback: GraphCallback,
    factory_key: String,
    node_type: String,
    output_port: PortName,
}

impl ForeignNodeProvider for TypeScriptProvider {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if config.is_empty() {
            Ok(())
        } else {
            Err(NodeFactoryError::new(
                "VOXA-NODE-GRAPH-CONFIG",
                "TypeScript Graph factory v1 accepts an empty node_config",
            ))
        }
    }

    fn create(
        &self,
        node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError> {
        Ok(Box::new(TypeScriptInstance {
            callback: self.callback.clone(),
            factory_key: self.factory_key.clone(),
            node_type: self.node_type.clone(),
            node_id: node_id.as_str().to_owned(),
            output_port: self.output_port.clone(),
        }))
    }
}

struct TypeScriptInstance {
    callback: GraphCallback,
    factory_key: String,
    node_type: String,
    node_id: String,
    output_port: PortName,
}

impl TypeScriptInstance {
    fn invoke(&self, kind: &str, payload_json: Option<String>) -> Result<GraphResponse, VoxaError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let status = self.callback.call_with_return_value::<String, _>(
            GraphCommand {
                factory_key: self.factory_key.clone(),
                node_type: self.node_type.clone(),
                node_id: self.node_id.clone(),
                kind: kind.to_owned(),
                payload_json,
            },
            ThreadsafeFunctionCallMode::NonBlocking,
            move |response| {
                let _ = sender.send(response);
                Ok(())
            },
        );
        if status != Status::Ok {
            return Err(graph_error(
                "VOXA-NODE-GRAPH-CALLBACK",
                "TypeScript callback queue is full or closing",
            ));
        }
        let encoded = receiver.recv_timeout(CALLBACK_TIMEOUT).map_err(|_| {
            graph_error(
                "VOXA-NODE-GRAPH-DEADLINE",
                "TypeScript lifecycle callback exceeded its deadline",
            )
        })?;
        let response: GraphResponse = serde_json::from_str(&encoded).map_err(|_| {
            graph_error(
                "VOXA-NODE-GRAPH-RESPONSE",
                "TypeScript callback returned an invalid response envelope",
            )
        })?;
        if response.ok {
            Ok(response)
        } else {
            let error = response.error.unwrap_or(GraphResponseError {
                code: "VOXA-NODE-EXCEPTION".to_owned(),
                message: "TypeScript callback failed".to_owned(),
            });
            Err(graph_error(&error.code, &error.message))
        }
    }
}

impl ForeignNodeInstance for TypeScriptInstance {
    fn on_prepare(&mut self) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.invoke("prepare", None)?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        _input_port: Option<&PortName>,
    ) -> Result<ForeignNodeCallOutput, VoxaError> {
        let input = input.ok_or_else(|| {
            graph_error(
                "VOXA-NODE-GRAPH-INPUT",
                "TypeScript transform requires one input frame",
            )
        })?;
        let text = input.as_text().ok_or_else(|| {
            graph_error(
                "VOXA-NODE-GRAPH-FRAME-TYPE",
                "TypeScript Graph factory v1 accepts text frames",
            )
        })?;
        let payload = serde_json::json!({ "text": text.data().as_str() }).to_string();
        let response = self.invoke("process", Some(payload))?;
        let value = response.value.ok_or_else(|| {
            graph_error(
                "VOXA-NODE-GRAPH-OUTPUT",
                "TypeScript transform returned no output",
            )
        })?;
        let text = value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                graph_error(
                    "VOXA-NODE-GRAPH-OUTPUT",
                    "TypeScript transform output must be an object with a text string",
                )
            })?;
        let frame = owned_text_frame(
            text.to_owned(),
            i64::try_from(input.header().sequence_id().get()).unwrap_or(i64::MAX),
        )
        .map_err(|error| graph_error("VOXA-NODE-GRAPH-OUTPUT", &error.to_string()))?;
        Ok(ForeignNodeCallOutput::from_frame(
            self.output_port.clone(),
            frame,
        ))
    }

    fn on_signal(&mut self, _signal: SignalFrame) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.invoke("signal", None)?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_finish(&mut self) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.invoke("finish", None)?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_abort(&mut self, reason: &voxa_core::AbortReason) {
        let payload = serde_json::json!({
            "code": reason.root().code(),
            "message": reason.root().message()
        })
        .to_string();
        let _ = self.invoke("abort", Some(payload));
    }
}

#[derive(Deserialize)]
struct GraphResponse {
    ok: bool,
    value: Option<serde_json::Value>,
    error: Option<GraphResponseError>,
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
        let mut registry = voxa_graph_json::builtin_registry();
        for spec in &self.factories {
            registry
                .register(registration(spec, self.callback.clone()))
                .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?;
        }
        let document = voxa_graph_json::parse(&self.graph_json).map_err(diagnostics)?;
        let graph =
            voxa_graph_json::compile_with_registry(&document, &registry).map_err(diagnostics)?;
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
                        "VOXA-NODE-GRAPH-TIMEOUT: active nodes [{}]",
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
    callback: JsFunction,
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
    let callback = callback.create_threadsafe_function(
        CALLBACK_QUEUE_CAPACITY,
        |context: ThreadSafeCallContext<GraphCommand>| {
            Ok(vec![JsGraphCommand {
                factory_key: context.value.factory_key,
                node_type: context.value.node_type,
                node_id: context.value.node_id,
                kind: context.value.kind,
                payload_json: context.value.payload_json,
            }])
        },
    )?;
    Ok(AsyncTask::new(RunRegisteredGraphTask {
        graph_json,
        factories,
        callback,
        timeout: Duration::from_millis(timeout_ms.into()),
    }))
}

fn parse_spec(spec: GraphFactorySpec) -> napi::Result<FactorySpec> {
    Ok(FactorySpec {
        node_type: NodeTypeName::new(spec.node_type)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
        version: NodeFactoryVersion::new(spec.version)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
        input_port: PortName::new(spec.input_port)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
        output_port: PortName::new(spec.output_port)
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?,
    })
}

fn registration(spec: &FactorySpec, callback: GraphCallback) -> NodeRegistration {
    let template = NodeId::new(format!("template-{}", spec.node_type.as_str()))
        .expect("valid node type produces valid template ID");
    let descriptor = NodeDescriptor::new(
        template.clone(),
        spec.node_type.clone(),
        NodeKind::Transform,
        [
            PortDescriptor::new(
                template.clone(),
                spec.input_port.clone(),
                PortDirection::Input,
                FrameType::Text,
            ),
            PortDescriptor::new(
                template,
                spec.output_port.clone(),
                PortDirection::Output,
                FrameType::Text,
            ),
        ],
        ConfigSchema::empty(),
        LifecycleCapabilities::new(true, true, true, true),
    );
    let provider = TypeScriptProvider {
        callback,
        factory_key: format!("{}@{}", spec.node_type.as_str(), spec.version.as_str()),
        node_type: spec.node_type.as_str().to_owned(),
        output_port: spec.output_port.clone(),
    };
    NodeRegistration::new(
        NodeLanguage::TypeScript,
        descriptor,
        spec.version.clone(),
        Arc::new(ForeignNodeFactoryAdapter::new(Arc::new(provider))),
    )
}

fn diagnostics(errors: Vec<voxa_graph_json::GraphDiagnostic>) -> Error {
    Error::new(
        Status::InvalidArg,
        errors
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn graph_error(code: &str, message: &str) -> VoxaError {
    VoxaError::try_new(ErrorCategory::External, code.to_owned(), message.to_owned()).unwrap_or_else(
        |_| {
            VoxaError::new(
                ErrorCategory::External,
                "VOXA-NODE-GRAPH",
                "TypeScript Graph execution failed",
            )
        },
    )
}
