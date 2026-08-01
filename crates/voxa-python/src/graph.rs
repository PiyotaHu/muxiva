//! Registry-driven Graph v1 execution hosted by Python.

use std::{sync::Arc, time::Duration};

use pyo3::prelude::*;
use voxa_core::{
    start_registered_runtime, AbortReason, ConfigMap, ConfigSchema, EdgePolicies,
    ForeignCommandKind, ForeignCompletionKind, ForeignNodeCallOutput, ForeignNodeEmission,
    ForeignNodeFactoryAdapter, ForeignNodeInstance, ForeignNodeProvider, LifecycleCapabilities,
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
    input_port: PortName,
    output_port: PortName,
    constructor: Py<PyAny>,
}

#[pymethods]
impl PyGraphNodeFactory {
    #[new]
    #[pyo3(signature = (node_type, constructor, *, version="1.0.0", input_port="text_in", output_port="text_out"))]
    fn new(
        node_type: String,
        constructor: Py<PyAny>,
        version: &str,
        input_port: &str,
        output_port: &str,
    ) -> PyResult<Self> {
        Ok(Self {
            node_type: NodeTypeName::new(node_type)
                .map_err(|error| binding_error("VOXA-PY-GRAPH-NODE-TYPE", error.to_string()))?,
            version: NodeFactoryVersion::new(version).map_err(|error| {
                binding_error("VOXA-PY-GRAPH-FACTORY-VERSION", error.to_string())
            })?,
            input_port: PortName::new(input_port)
                .map_err(|error| binding_error("VOXA-PY-GRAPH-PORT", error.to_string()))?,
            output_port: PortName::new(output_port)
                .map_err(|error| binding_error("VOXA-PY-GRAPH-PORT", error.to_string()))?,
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
            NodeKind::Transform,
            [
                PortDescriptor::new(
                    template.clone(),
                    self.input_port.clone(),
                    PortDirection::Input,
                    FrameType::Text,
                ),
                PortDescriptor::new(
                    template,
                    self.output_port.clone(),
                    PortDirection::Output,
                    FrameType::Text,
                ),
            ],
            ConfigSchema::empty(),
            LifecycleCapabilities::new(true, true, true, true),
        );
        let provider = PythonProvider {
            constructor: self.constructor.clone_ref(py),
            output_port: self.output_port.clone(),
        };
        NodeRegistration::new(
            NodeLanguage::Python,
            descriptor,
            self.version.clone(),
            Arc::new(ForeignNodeFactoryAdapter::new(Arc::new(provider))),
        )
    }
}

struct PythonProvider {
    constructor: Py<PyAny>,
    output_port: PortName,
}

impl ForeignNodeProvider for PythonProvider {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if config.is_empty() {
            Ok(())
        } else {
            Err(NodeFactoryError::new(
                "VOXA-PY-GRAPH-CONFIG",
                "Python Graph factory v1 accepts an empty node_config",
            ))
        }
    }

    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError> {
        let node = Python::with_gil(|py| self.constructor.call0(py)).map_err(|error| {
            NodeFactoryError::new("VOXA-PY-GRAPH-CONSTRUCTOR", error.to_string())
        })?;
        let domain =
            PythonNodeExecutionDomain::new(node, 16, 16, 1, 10_000, 5_000, "strict", "in_process")
                .map_err(|error| {
                    NodeFactoryError::new("VOXA-PY-GRAPH-DOMAIN", error.to_string())
                })?;
        Ok(Box::new(PythonInstance {
            domain,
            output_port: self.output_port.clone(),
        }))
    }
}

struct PythonInstance {
    domain: PythonNodeExecutionDomain,
    output_port: PortName,
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
        _input_port: Option<&PortName>,
    ) -> Result<ForeignNodeCallOutput, VoxaError> {
        let input = input.ok_or_else(|| {
            graph_error(
                "VOXA-PY-GRAPH-INPUT",
                "Python transform requires one input frame",
            )
        })?;
        let completion = self
            .domain
            .submit_blocking(ForeignCommandKind::Process(input))?;
        let ForeignCompletionKind::Success { frames, .. } = completion.kind() else {
            unreachable!("domain failures are returned as errors")
        };
        Ok(ForeignNodeCallOutput::new(
            frames
                .iter()
                .cloned()
                .map(|frame| ForeignNodeEmission::new(self.output_port.clone(), frame))
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
    match py.allow_threads(|| runtime.wait(Duration::from_millis(timeout_ms))) {
        Ok(summary) => Ok(summary.worker_total()),
        Err(RuntimeWaitError::Aborted(reason)) => {
            Err(binding_error(reason.root().code(), reason.root().message()))
        }
        Err(RuntimeWaitError::Timeout(diagnostics)) => {
            runtime.stop();
            let _ = py.allow_threads(|| runtime.wait(Duration::from_secs(5)));
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
