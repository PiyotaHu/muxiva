//! Registry-driven Graph v1 execution hosted by Python.

use std::{sync::Arc, time::Duration};

use pyo3::prelude::*;
use serde::Deserialize;
use voxa_core::{
    start_registered_runtime, AbortReason, ConfigMap, ConfigSchema, EdgePolicies,
    ForeignCommandKind, ForeignCompletionKind, ForeignNodeCallOutput, ForeignNodeConstructor,
    ForeignNodeEmission, ForeignNodeFactoryAdapter, ForeignNodeInstance, LifecycleCapabilities,
    NodeDescriptor, NodeFactoryError, NodeFactoryVersion, NodeKind, NodeLanguage, NodeRegistration,
    NodeTypeName, PortDescriptor, PortDirection, PortName, RuntimeOptions, RuntimeWaitError,
};
use voxa_types::{ErrorCategory, Frame, FrameType, NodeId, SignalFrame, VoxaError};

use crate::{binding_error, domain::PythonNodeExecutionDomain};

const MAX_TIMEOUT_MS: u64 = 60 * 60 * 1_000;

#[pyclass(frozen, name = "GraphNodeFactory")]
pub struct PyGraphNodeFactory {
    node_type: NodeTypeName,
    version: NodeFactoryVersion,
    kind: NodeKind,
    ports: Vec<FactoryPort>,
    config_schema: ConfigSchema,
    pass_config: bool,
    constructor: Py<PyAny>,
}

#[derive(Clone)]
struct FactoryPort {
    name: PortName,
    direction: PortDirection,
    frame_type: FrameType,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FactoryPortDocument {
    name: String,
    direction: String,
    frame_type: String,
}

#[pymethods]
impl PyGraphNodeFactory {
    #[new]
    #[pyo3(signature = (node_type, constructor, *, version="1.0.0", input_port="text_in", output_port="text_out", kind="transform", ports_json=None, config_schema_json="{}", pass_config=false))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        node_type: String,
        constructor: Py<PyAny>,
        version: &str,
        input_port: &str,
        output_port: &str,
        kind: &str,
        ports_json: Option<&str>,
        config_schema_json: &str,
        pass_config: bool,
    ) -> PyResult<Self> {
        let kind = parse_kind(kind)?;
        let ports = match ports_json {
            Some(value) => parse_ports(value)?,
            None => default_text_ports(input_port, output_port)?,
        };
        let schema_json: serde_json::Value = serde_json::from_str(config_schema_json)
            .map_err(|error| binding_error("VOXA-PY-GRAPH-SCHEMA", error.to_string()))?;
        Ok(Self {
            node_type: NodeTypeName::new(node_type)
                .map_err(|error| binding_error("VOXA-PY-GRAPH-NODE-TYPE", error.to_string()))?,
            version: NodeFactoryVersion::new(version).map_err(|error| {
                binding_error("VOXA-PY-GRAPH-FACTORY-VERSION", error.to_string())
            })?,
            kind,
            ports,
            config_schema: ConfigSchema::new(
                voxa_graph_json::value_from_json(&schema_json)
                    .map_err(|error| binding_error("VOXA-PY-GRAPH-SCHEMA", error))?,
            ),
            pass_config,
            constructor,
        })
    }

    #[getter]
    fn node_type(&self) -> &str {
        self.node_type.as_str()
    }

    #[getter]
    fn version(&self) -> &str {
        self.version.as_str()
    }
}

impl PyGraphNodeFactory {
    fn registration(&self, py: Python<'_>) -> NodeRegistration {
        let template = NodeId::new(format!("template-{}", self.node_type.as_str()))
            .expect("valid node type produces valid template ID");
        let descriptor = NodeDescriptor::new(
            template.clone(),
            self.node_type.clone(),
            self.kind,
            self.ports
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
            self.config_schema.clone(),
            LifecycleCapabilities::new(true, true, true, true),
        );
        let constructor = PythonNodeConstructor {
            constructor: self.constructor.clone_ref(py),
            output_ports: self
                .ports
                .iter()
                .filter(|port| port.direction == PortDirection::Output)
                .map(|port| port.name.clone())
                .collect(),
            pass_config: self.pass_config,
        };
        NodeRegistration::new(
            NodeLanguage::Python,
            descriptor,
            self.version.clone(),
            Arc::new(ForeignNodeFactoryAdapter::new(Arc::new(constructor))),
        )
    }
}

struct PythonNodeConstructor {
    constructor: Py<PyAny>,
    output_ports: Vec<PortName>,
    pass_config: bool,
}

impl ForeignNodeConstructor for PythonNodeConstructor {
    fn create(
        &self,
        _node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError> {
        let node = Python::attach(|py| {
            if self.pass_config {
                let json = py.import("json")?;
                let encoded = voxa_graph_json::config_map_to_json(config).to_string();
                let value = json.call_method1("loads", (encoded,))?;
                self.constructor.call1(py, (value,))
            } else {
                self.constructor.call0(py)
            }
        })
        .map_err(|error| NodeFactoryError::new("VOXA-PY-GRAPH-CONSTRUCTOR", error.to_string()))?;
        let domain = PythonNodeExecutionDomain::new_graph(node, self.output_ports.clone())
            .map_err(|error| NodeFactoryError::new("VOXA-PY-GRAPH-DOMAIN", error.to_string()))?;
        Ok(Box::new(PythonInstance { domain }))
    }
}

struct PythonInstance {
    domain: PythonNodeExecutionDomain,
}

impl ForeignNodeInstance for PythonInstance {
    fn on_prepare(&mut self) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.domain
            .submit_blocking(ForeignCommandKind::Prepare)
            .map(|_| ForeignNodeCallOutput::default())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        input_port: Option<&PortName>,
    ) -> Result<ForeignNodeCallOutput, VoxaError> {
        let command = match input {
            Some(frame) => ForeignCommandKind::ProcessOnPort {
                frame,
                input_port: input_port.cloned().ok_or_else(|| {
                    graph_error("VOXA-PY-GRAPH-INPUT-PORT", "input port identity is missing")
                })?,
            },
            None => ForeignCommandKind::ProcessSource,
        };
        let completion = self.domain.submit_blocking(command)?;
        let ForeignCompletionKind::Success { emissions, .. } = completion.kind() else {
            unreachable!("domain failures are returned as errors")
        };
        Ok(ForeignNodeCallOutput::new(
            emissions
                .iter()
                .map(|emission| {
                    ForeignNodeEmission::new(
                        emission.output_port().clone(),
                        emission.frame().clone(),
                    )
                })
                .collect::<Vec<_>>(),
            [],
        ))
    }

    fn on_signal(&mut self, signal: SignalFrame) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.domain
            .submit_blocking(ForeignCommandKind::Signal(signal))
            .map(|_| ForeignNodeCallOutput::default())
    }

    fn on_finish(&mut self) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.domain.submit_blocking(ForeignCommandKind::Finish)?;
        self.domain.mark_terminal_callback_completed();
        self.domain.close_blocking()?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_abort(&mut self, reason: &AbortReason) {
        let _ = self
            .domain
            .submit_blocking(ForeignCommandKind::Abort(reason.clone()));
        self.domain.mark_terminal_callback_completed();
        let _ = self.domain.close_blocking();
    }
}

#[pyfunction]
#[pyo3(signature = (graph_json, factories, *, timeout_ms=30_000))]
pub fn run_graph(
    py: Python<'_>,
    graph_json: &str,
    factories: Vec<Py<PyGraphNodeFactory>>,
    timeout_ms: u64,
) -> PyResult<usize> {
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return Err(binding_error(
            "VOXA-PY-GRAPH-TIMEOUT",
            format!("timeout_ms must be between 1 and {MAX_TIMEOUT_MS}"),
        ));
    }
    let mut registry = voxa_graph_json::builtin_registry();
    for factory in factories {
        registry
            .register(factory.borrow(py).registration(py))
            .map_err(|error| binding_error("VOXA-PY-GRAPH-REGISTRY", error.to_string()))?;
    }
    let document = voxa_graph_json::parse(graph_json).map_err(graph_diagnostics)?;
    let graph =
        voxa_graph_json::compile_with_registry(&document, &registry).map_err(graph_diagnostics)?;
    let runtime = start_registered_runtime(
        graph,
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .map_err(|error| binding_error("VOXA-PY-GRAPH-START", error.to_string()))?;
    match py.detach(|| runtime.wait(Duration::from_millis(timeout_ms))) {
        Ok(summary) => Ok(summary.worker_total()),
        Err(RuntimeWaitError::Aborted(reason)) => {
            Err(binding_error(reason.root().code(), reason.root().message()))
        }
        Err(RuntimeWaitError::Timeout(diagnostics)) => {
            runtime.stop();
            let _ = py.detach(|| runtime.wait(Duration::from_secs(5)));
            Err(binding_error(
                "VOXA-PY-GRAPH-TIMEOUT",
                format!(
                    "graph timed out with active nodes [{}]",
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

fn graph_diagnostics(diagnostics: Vec<voxa_graph_json::GraphDiagnostic>) -> PyErr {
    binding_error(
        "VOXA-PY-GRAPH-COMPILE",
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn graph_error(code: &str, message: &str) -> VoxaError {
    VoxaError::new(ErrorCategory::External, code, message)
}

fn parse_kind(value: &str) -> PyResult<NodeKind> {
    match value {
        "source" => Ok(NodeKind::Source),
        "transform" => Ok(NodeKind::Transform),
        "sink" => Ok(NodeKind::Sink),
        _ => Err(binding_error(
            "VOXA-PY-GRAPH-KIND",
            "kind must be source, transform, or sink",
        )),
    }
}

fn default_text_ports(input: &str, output: &str) -> PyResult<Vec<FactoryPort>> {
    Ok(vec![
        FactoryPort {
            name: PortName::new(input)
                .map_err(|error| binding_error("VOXA-PY-GRAPH-PORT", error.to_string()))?,
            direction: PortDirection::Input,
            frame_type: FrameType::Text,
        },
        FactoryPort {
            name: PortName::new(output)
                .map_err(|error| binding_error("VOXA-PY-GRAPH-PORT", error.to_string()))?,
            direction: PortDirection::Output,
            frame_type: FrameType::Text,
        },
    ])
}

fn parse_ports(encoded: &str) -> PyResult<Vec<FactoryPort>> {
    let documents: Vec<FactoryPortDocument> = serde_json::from_str(encoded)
        .map_err(|error| binding_error("VOXA-PY-GRAPH-PORTS", error.to_string()))?;
    documents
        .into_iter()
        .map(|document| {
            let direction = match document.direction.as_str() {
                "input" => PortDirection::Input,
                "output" => PortDirection::Output,
                _ => {
                    return Err(binding_error(
                        "VOXA-PY-GRAPH-PORT-DIRECTION",
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
                    return Err(binding_error(
                        "VOXA-PY-GRAPH-FRAME-TYPE",
                        "port frame_type must be audio, video, text, or byte",
                    ))
                }
            };
            Ok(FactoryPort {
                name: PortName::new(document.name)
                    .map_err(|error| binding_error("VOXA-PY-GRAPH-PORT", error.to_string()))?,
                direction,
                frame_type,
            })
        })
        .collect()
}
