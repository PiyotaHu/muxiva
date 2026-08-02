use libloading::Library;
use std::{
    collections::BTreeMap,
    ffi::c_void,
    mem,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use voxa_core::{
    start_registered_runtime, AbortReason, ConfigMap, ConfigSchema, EdgeDescriptor, EdgePolicies,
    EnabledCondition, ForeignNodeCallOutput, ForeignNodeFactoryAdapter, ForeignNodeInstance,
    ForeignNodeProvider, GraphBuilder, GraphRunner, LifecycleCapabilities, Node, NodeContext,
    NodeDescriptor, NodeFactoryError, NodeFactoryVersion, NodeInstances, NodeKind, NodeLanguage,
    NodeRegistration, NodeTypeName, PortDescriptor, PortDirection, PortName, QueuePolicy,
    RuntimeOptions, RuntimeWaitError, TransformPolicy, ValidationPolicy, VisibilityDescriptor,
};
use voxa_types::{EdgeId, ErrorCategory, Frame, FrameType, NodeId, SignalFrame, VoxaError};

use crate::{
    abi::{
        self, AbortReasonView, ErrorOutput, FactoryCreateCallback, GraphFactoryCreateCallback,
        GraphNodeVtable, NodeVtable, StrView,
    },
    error::FfiError,
    frame::{borrowed_frame_view, borrowed_text_view, copy_frame},
};

#[derive(Clone)]
pub struct CppFactorySpec {
    pub node_type: NodeTypeName,
    pub version: NodeFactoryVersion,
    pub input_port: PortName,
    pub output_port: PortName,
    pub user_data: usize,
    pub create: FactoryCreateCallback,
}

#[derive(Clone)]
pub struct CppMultimodalFactorySpec {
    pub node_type: NodeTypeName,
    pub version: NodeFactoryVersion,
    pub kind: NodeKind,
    pub ports: Vec<(PortName, PortDirection, FrameType)>,
    pub config_schema: ConfigSchema,
    pub user_data: usize,
    pub create: GraphFactoryCreateCallback,
}

struct CppProvider {
    spec: CppFactorySpec,
}

impl ForeignNodeProvider for CppProvider {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        if config.is_empty() {
            Ok(())
        } else {
            Err(NodeFactoryError::new(
                "VOXA-FFI-GRAPH-CONFIG",
                "C++ Graph factory v1 accepts an empty node_config",
            ))
        }
    }

    fn create(
        &self,
        node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError> {
        let mut table = empty_node_vtable();
        let mut output = empty_error();
        let status = (self.spec.create)(
            self.spec.user_data as *mut c_void,
            str_view(node_id.as_str()),
            &mut table,
            &mut output,
        );
        if status != abi::OK {
            return Err(factory_callback_error(&output));
        }
        if let Err(error) = validate_node_vtable(&table) {
            if let Some(destroy) = table.destroy {
                destroy(table.user_data);
            }
            return Err(error);
        }
        Ok(Box::new(CppInstance {
            record: NodeRecord::new(table),
            output_port: self.spec.output_port.clone(),
        }))
    }
}

struct CppInstance {
    record: NodeRecord,
    output_port: PortName,
}

impl ForeignNodeInstance for CppInstance {
    fn on_prepare(&mut self) -> voxa_types::Result<ForeignNodeCallOutput> {
        self.record.prepare()?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        _input_port: Option<&PortName>,
    ) -> voxa_types::Result<ForeignNodeCallOutput> {
        let input = input.ok_or_else(|| {
            foreign_error(
                abi::INVALID_ARGUMENT,
                "VOXA-FFI-GRAPH-INPUT",
                "C++ transform requires one input frame",
            )
        })?;
        let output = self.record.process(&input)?;
        Ok(ForeignNodeCallOutput::from_frame(
            self.output_port.clone(),
            output,
        ))
    }

    fn on_signal(&mut self, signal: SignalFrame) -> voxa_types::Result<ForeignNodeCallOutput> {
        self.record.signal(&signal)?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_finish(&mut self) -> voxa_types::Result<ForeignNodeCallOutput> {
        self.record.finish()?;
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_abort(&mut self, reason: &AbortReason) {
        self.record.abort(reason);
    }
}

pub fn run_registered_graph(
    graph_json: &str,
    specs: &[CppFactorySpec],
    timeout: Duration,
) -> Result<usize, FfiError> {
    let mut registry = voxa_graph_json::builtin_registry();
    for spec in specs {
        registry
            .register(cpp_registration(spec.clone()))
            .map_err(|_| {
                FfiError::validation(
                    "VOXA-FFI-GRAPH-REGISTRY",
                    "invalid or duplicate C++ Graph factory",
                )
            })?;
    }
    let document = voxa_graph_json::parse(graph_json)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-JSON", "Graph v1 parsing failed"))?;
    let graph = voxa_graph_json::compile_with_registry(&document, &registry).map_err(|_| {
        FfiError::validation("VOXA-FFI-GRAPH-COMPILE", "Graph v1 compilation failed")
    })?;
    let runtime = start_registered_runtime(
        graph,
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .map_err(|_| FfiError::internal("VOXA-FFI-GRAPH-START", "Graph runtime startup failed"))?;
    match runtime.wait(timeout) {
        Ok(summary) => Ok(summary.worker_total()),
        Err(RuntimeWaitError::Aborted(_)) => Err(FfiError {
            status: abi::EXTERNAL,
            category: 4,
            code: "VOXA-FFI-GRAPH-ABORT",
            message: "Graph runtime aborted",
        }),
        Err(RuntimeWaitError::Timeout(_)) => {
            runtime.stop();
            let _ = runtime.wait(Duration::from_secs(5));
            Err(FfiError {
                status: abi::TIMEOUT,
                category: 3,
                code: "VOXA-FFI-GRAPH-TIMEOUT",
                message: "Graph runtime exceeded its deadline",
            })
        }
    }
}

fn cpp_registration(spec: CppFactorySpec) -> NodeRegistration {
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
    NodeRegistration::new(
        NodeLanguage::Cpp,
        descriptor,
        spec.version.clone(),
        Arc::new(ForeignNodeFactoryAdapter::new(Arc::new(CppProvider {
            spec,
        }))),
    )
}

struct CppMultimodalProvider {
    spec: CppMultimodalFactorySpec,
    library: Option<Arc<Library>>,
}

impl ForeignNodeProvider for CppMultimodalProvider {
    fn create(
        &self,
        node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError> {
        let mut table = GraphNodeVtable {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<GraphNodeVtable>()).unwrap_or(u32::MAX),
            user_data: std::ptr::null_mut(),
            on_prepare: None,
            on_process: None,
            on_signal: None,
            on_finish: None,
            on_abort: None,
            destroy: None,
            capabilities: 0,
            reserved: [0; 3],
        };
        let mut output = empty_error();
        let config_json = voxa_graph_json::config_map_to_json(config).to_string();
        let status = (self.spec.create)(
            self.spec.user_data as *mut c_void,
            str_view(node_id.as_str()),
            str_view(&config_json),
            &mut table,
            &mut output,
        );
        if status != abi::OK {
            return Err(factory_callback_error(&output));
        }
        let expected = u32::try_from(mem::size_of::<GraphNodeVtable>()).unwrap_or(u32::MAX);
        if table.abi_version != abi::ABI_VERSION
            || table.struct_size != expected
            || table.reserved != [0; 3]
            || table.on_process.is_none()
        {
            if let Some(destroy) = table.destroy {
                destroy(table.user_data);
            }
            return Err(NodeFactoryError::new(
                "VOXA-FFI-GRAPH-VTABLE",
                "C++ multimodal factory returned an invalid Graph Node vtable",
            ));
        }
        Ok(Box::new(CppMultimodalInstance {
            table,
            _library: self.library.clone(),
        }))
    }
}

struct CppMultimodalInstance {
    table: GraphNodeVtable,
    // Keep callback code mapped until after `destroy` runs in Drop.
    _library: Option<Arc<Library>>,
}

impl ForeignNodeInstance for CppMultimodalInstance {
    fn on_prepare(&mut self) -> voxa_types::Result<ForeignNodeCallOutput> {
        if let Some(callback) = self.table.on_prepare {
            call_simple(callback, self.table.user_data)?;
        }
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        input_port: Option<&PortName>,
    ) -> voxa_types::Result<ForeignNodeCallOutput> {
        let callback = self.table.on_process.expect("validated graph callback");
        let input_view = input
            .as_ref()
            .map(borrowed_frame_view)
            .transpose()
            .map_err(to_voxa_error)?;
        let input_pointer = input_view
            .as_ref()
            .map_or(std::ptr::null(), std::ptr::from_ref);
        let port_view = input_port.map_or(
            StrView {
                data: std::ptr::null(),
                len: 0,
            },
            |port| str_view(port.as_str()),
        );
        let mut outputs = std::ptr::null();
        let mut output_count = 0_usize;
        let mut error = empty_error();
        let status = callback(
            self.table.user_data,
            input_pointer,
            port_view,
            &mut outputs,
            &mut output_count,
            &mut error,
        );
        if status != abi::OK {
            return Err(callback_error(status, &error));
        }
        if output_count > 4_096 || (output_count != 0 && !abi::aligned(outputs)) {
            return Err(foreign_error(
                abi::INVALID_ARGUMENT,
                "VOXA-FFI-GRAPH-OUTPUT",
                "C++ callback returned an invalid emission array",
            ));
        }
        let views = if output_count == 0 {
            &[]
        } else {
            // SAFETY: callback contract keeps output_count entries borrowed until callback return;
            // the entries are copied synchronously before the next foreign call.
            unsafe { std::slice::from_raw_parts(outputs, output_count) }
        };
        let mut emissions = Vec::with_capacity(views.len());
        for view in views {
            let name = abi::copy_str(view.output_port, true).map_err(|_| {
                to_voxa_error(FfiError::validation(
                    "VOXA-FFI-GRAPH-OUTPUT",
                    "invalid output port",
                ))
            })?;
            let port = PortName::new(name).map_err(|_| {
                foreign_error(
                    abi::INVALID_ARGUMENT,
                    "VOXA-FFI-GRAPH-OUTPUT",
                    "invalid output port",
                )
            })?;
            let frame = copy_frame(&view.frame)
                .and_then(|frame| frame.to_rust())
                .map_err(to_voxa_error)?;
            emissions.push(voxa_core::ForeignNodeEmission::new(port, frame));
        }
        Ok(ForeignNodeCallOutput::new(emissions, []))
    }

    fn on_finish(&mut self) -> voxa_types::Result<ForeignNodeCallOutput> {
        if let Some(callback) = self.table.on_finish {
            call_simple(callback, self.table.user_data)?;
        }
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_abort(&mut self, reason: &AbortReason) {
        let Some(callback) = self.table.on_abort else {
            return;
        };
        let view = AbortReasonView {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<AbortReasonView>()).unwrap_or(u32::MAX),
            category: reason.category() as i32,
            stage: reason.stage() as i32,
            code: str_view(reason.root().code()),
            message: str_view(reason.root().message()),
        };
        callback(self.table.user_data, &view);
    }
}

impl Drop for CppMultimodalInstance {
    fn drop(&mut self) {
        if let Some(destroy) = self.table.destroy {
            destroy(self.table.user_data);
        }
    }
}

pub fn run_registered_multimodal_graph(
    graph_json: &str,
    specs: &[CppMultimodalFactorySpec],
    timeout: Duration,
) -> Result<usize, FfiError> {
    let mut registry = voxa_graph_json::builtin_registry();
    for spec in specs {
        registry
            .register(cpp_multimodal_registration(spec.clone(), None))
            .map_err(|_| {
                FfiError::validation(
                    "VOXA-FFI-GRAPH-REGISTRY",
                    "invalid or duplicate C++ multimodal factory",
                )
            })?;
    }
    let document = voxa_graph_json::parse(graph_json)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-JSON", "Graph v1 parsing failed"))?;
    let graph = voxa_graph_json::compile_with_registry(&document, &registry).map_err(|_| {
        FfiError::validation("VOXA-FFI-GRAPH-COMPILE", "Graph v1 compilation failed")
    })?;
    let runtime = start_registered_runtime(
        graph,
        &registry,
        EdgePolicies::new(),
        RuntimeOptions::default(),
    )
    .map_err(|_| FfiError::internal("VOXA-FFI-GRAPH-START", "Graph runtime startup failed"))?;
    match runtime.wait(timeout) {
        Ok(summary) => Ok(summary.worker_total()),
        Err(RuntimeWaitError::Aborted(_)) => Err(FfiError {
            status: abi::EXTERNAL,
            category: 4,
            code: "VOXA-FFI-GRAPH-ABORT",
            message: "Graph runtime aborted",
        }),
        Err(RuntimeWaitError::Timeout(_)) => {
            runtime.stop();
            let _ = runtime.wait(Duration::from_secs(5));
            Err(FfiError {
                status: abi::TIMEOUT,
                category: 3,
                code: "VOXA-FFI-GRAPH-TIMEOUT",
                message: "Graph runtime exceeded its deadline",
            })
        }
    }
}

pub fn cpp_multimodal_registration(
    spec: CppMultimodalFactorySpec,
    library: Option<Arc<Library>>,
) -> NodeRegistration {
    let template =
        NodeId::new(format!("template-{}", spec.node_type.as_str())).expect("valid template ID");
    let descriptor = NodeDescriptor::new(
        template.clone(),
        spec.node_type.clone(),
        spec.kind,
        spec.ports
            .iter()
            .map(|(name, direction, frame_type)| {
                PortDescriptor::new(template.clone(), name.clone(), *direction, *frame_type)
            })
            .collect::<Vec<_>>(),
        spec.config_schema.clone(),
        LifecycleCapabilities::new(true, true, true, true),
    );
    NodeRegistration::new(
        NodeLanguage::Cpp,
        descriptor,
        spec.version.clone(),
        Arc::new(ForeignNodeFactoryAdapter::new(Arc::new(
            CppMultimodalProvider { spec, library },
        ))),
    )
}

fn validate_node_vtable(table: &NodeVtable) -> Result<(), NodeFactoryError> {
    let expected = u32::try_from(mem::size_of::<NodeVtable>()).unwrap_or(u32::MAX);
    if table.abi_version != abi::ABI_VERSION || table.struct_size != expected {
        return Err(NodeFactoryError::new(
            "VOXA-FFI-GRAPH-VTABLE",
            "C++ factory returned a mismatched Node vtable",
        ));
    }
    if table.reserved != [0; 3] || table.on_process.is_none() {
        return Err(NodeFactoryError::new(
            "VOXA-FFI-GRAPH-VTABLE",
            "C++ factory returned an invalid Node vtable",
        ));
    }
    Ok(())
}

fn factory_callback_error(output: &ErrorOutput) -> NodeFactoryError {
    NodeFactoryError::new(
        c_string(&output.code).unwrap_or("VOXA-FFI-GRAPH-FACTORY"),
        c_string(&output.message).unwrap_or("C++ Graph factory creation failed"),
    )
}

fn empty_node_vtable() -> NodeVtable {
    NodeVtable {
        abi_version: abi::ABI_VERSION,
        struct_size: u32::try_from(mem::size_of::<NodeVtable>()).unwrap_or(u32::MAX),
        user_data: std::ptr::null_mut(),
        on_prepare: None,
        on_process: None,
        on_signal: None,
        on_finish: None,
        on_abort: None,
        destroy: None,
        capabilities: 0,
        reserved: [0; 3],
    }
}

pub struct NodeRecord {
    vtable: NodeVtable,
    active: AtomicUsize,
    running: AtomicBool,
    closed: AtomicBool,
    destroyed: AtomicBool,
}

impl NodeRecord {
    pub fn new(vtable: NodeVtable) -> Self {
        Self {
            vtable,
            active: AtomicUsize::new(0),
            running: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            destroyed: AtomicBool::new(false),
        }
    }

    pub fn begin_run(self: &Arc<Self>) -> Result<RunGuard, FfiError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(FfiError::handle(abi::CLOSED, "node is closing"));
        }
        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                FfiError::handle(abi::BUSY, "node is already participating in a graph run")
            })?;
        self.active.fetch_add(1, Ordering::AcqRel);
        if self.closed.load(Ordering::Acquire) {
            self.active.fetch_sub(1, Ordering::AcqRel);
            self.running.store(false, Ordering::Release);
            return Err(FfiError::handle(
                abi::CLOSED,
                "node closed while graph run was admitted",
            ));
        }
        Ok(RunGuard(self.clone()))
    }

    pub fn close_if_idle(&self) -> Result<(), FfiError> {
        self.closed.store(true, Ordering::Release);
        if self.active.load(Ordering::Acquire) == 0 {
            Ok(())
        } else {
            Err(FfiError::handle(
                abi::BUSY,
                "node has an in-flight graph run",
            ))
        }
    }

    fn prepare(&self) -> voxa_types::Result<()> {
        match self.vtable.on_prepare {
            Some(callback) => call_simple(callback, self.vtable.user_data),
            None => Ok(()),
        }
    }

    fn process(&self, input: &Frame) -> voxa_types::Result<Frame> {
        let callback = self.vtable.on_process.ok_or_else(|| {
            foreign_error(
                abi::INVALID_ARGUMENT,
                "VOXA-FFI-NODE-PROCESS",
                "node vtable has no on_process callback",
            )
        })?;
        let input_view = borrowed_text_view(input).map_err(to_voxa_error)?;
        let mut output_view = abi::empty_frame_view();
        let mut output_error = empty_error();
        let status = callback(
            self.vtable.user_data,
            &input_view,
            &mut output_view,
            &mut output_error,
        );
        if status != abi::OK {
            return Err(callback_error(status, &output_error));
        }
        copy_frame(&output_view)
            .and_then(|frame| frame.to_rust_text())
            .map_err(to_voxa_error)
    }

    fn signal(&self, signal: &SignalFrame) -> voxa_types::Result<()> {
        let Some(callback) = self.vtable.on_signal else {
            return Ok(());
        };
        let frame = Frame::Signal(signal.clone());
        let view = borrowed_text_view(&frame).map_err(to_voxa_error)?;
        let mut output_error = empty_error();
        let status = callback(self.vtable.user_data, &view, &mut output_error);
        if status == abi::OK {
            Ok(())
        } else {
            Err(callback_error(status, &output_error))
        }
    }

    fn finish(&self) -> voxa_types::Result<()> {
        match self.vtable.on_finish {
            Some(callback) => call_simple(callback, self.vtable.user_data),
            None => Ok(()),
        }
    }

    fn abort(&self, reason: &AbortReason) {
        let Some(callback) = self.vtable.on_abort else {
            return;
        };
        let code = reason.root().code();
        let message = reason.root().message();
        let view = AbortReasonView {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<AbortReasonView>()).unwrap_or(u32::MAX),
            category: reason.category() as i32,
            stage: reason.stage() as i32,
            code: str_view(code),
            message: str_view(message),
        };
        callback(self.vtable.user_data, &view);
    }
}

impl Drop for NodeRecord {
    fn drop(&mut self) {
        if !self.destroyed.swap(true, Ordering::AcqRel) {
            if let Some(destroy) = self.vtable.destroy {
                destroy(self.vtable.user_data);
            }
        }
    }
}

pub struct RunGuard(Arc<NodeRecord>);

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
        self.0.running.store(false, Ordering::Release);
    }
}

struct ForeignNode {
    record: Arc<NodeRecord>,
}

impl Node for ForeignNode {
    fn on_prepare(&mut self, _context: &mut NodeContext) -> voxa_types::Result<()> {
        self.record.prepare()
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            foreign_error(
                abi::INTERNAL,
                "VOXA-FFI-NODE-INPUT",
                "foreign transform requires an input frame",
            )
        })?;
        let output = self.record.process(&input)?;
        context.emit(port("out"), output)?;
        Ok(())
    }

    fn on_signal(
        &mut self,
        signal: SignalFrame,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        self.record.signal(&signal)
    }

    fn on_finish(&mut self, _context: &mut NodeContext) -> voxa_types::Result<()> {
        self.record.finish()
    }

    fn on_abort(&mut self, reason: &AbortReason, _context: &mut NodeContext) {
        self.record.abort(reason);
    }
}

struct Source {
    frame: Frame,
}
impl Node for Source {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        debug_assert!(input.is_none());
        context.emit(port("out"), self.frame.clone())?;
        Ok(())
    }
}

struct Sink {
    output: Arc<Mutex<Option<String>>>,
}
impl Node for Sink {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.ok_or_else(|| {
            foreign_error(
                abi::INTERNAL,
                "VOXA-FFI-SINK-INPUT",
                "sink requires an input frame",
            )
        })?;
        let text = input.as_text().ok_or_else(|| {
            foreign_error(abi::INTERNAL, "VOXA-FFI-SINK-TYPE", "sink requires text")
        })?;
        *self
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(text.data().as_str().to_owned());
        Ok(())
    }
}

pub fn run_text_graph(record: Arc<NodeRecord>, input: Frame) -> Result<String, FfiError> {
    let _guard = record.begin_run()?;
    let graph = graph().map_err(|_| {
        FfiError::internal("VOXA-FFI-GRAPH", "failed to build focused bridge graph")
    })?;
    let output = Arc::new(Mutex::new(None));
    let nodes: NodeInstances = BTreeMap::from([
        (
            node_id("ffi-source"),
            Box::new(Source { frame: input }) as Box<dyn Node>,
        ),
        (
            node_id("ffi-transform"),
            Box::new(ForeignNode { record }) as Box<dyn Node>,
        ),
        (
            node_id("ffi-sink"),
            Box::new(Sink {
                output: output.clone(),
            }) as Box<dyn Node>,
        ),
    ]);
    let mut runner = GraphRunner::new(&graph, nodes, BTreeMap::new())
        .map_err(|_| FfiError::internal("VOXA-FFI-GRAPH", "failed to attach bridge node"))?;
    runner.run().map_err(|reason| {
        let status = if reason.root().code() == "VOXA-FFI-CPP-EXCEPTION" {
            abi::FOREIGN_EXCEPTION
        } else {
            abi::EXTERNAL
        };
        FfiError {
            status,
            category: if status == abi::FOREIGN_EXCEPTION {
                5
            } else {
                4
            },
            code: "VOXA-FFI-NODE",
            message: "foreign node aborted the graph",
        }
    })?;
    let result = output
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take()
        .ok_or_else(|| {
            FfiError::internal("VOXA-FFI-NO-OUTPUT", "foreign transform emitted no text")
        });
    result
}

fn graph() -> Result<voxa_core::GraphDefinition, voxa_core::GraphBuildError> {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(descriptor(
            "ffi-source",
            "voxa.ffi.source",
            NodeKind::Source,
            [("out", PortDirection::Output)],
        ))?
        .add_node(descriptor(
            "ffi-transform",
            "voxa.ffi.foreign",
            NodeKind::Transform,
            [("in", PortDirection::Input), ("out", PortDirection::Output)],
        ))?
        .add_node(descriptor(
            "ffi-sink",
            "voxa.ffi.sink",
            NodeKind::Sink,
            [("in", PortDirection::Input)],
        ))?
        .connect(edge(
            "ffi-source-transform",
            "ffi-source",
            "out",
            "ffi-transform",
            "in",
        ))?
        .connect(edge(
            "ffi-transform-sink",
            "ffi-transform",
            "out",
            "ffi-sink",
            "in",
        ))?;
    builder.build()
}

fn descriptor<const N: usize>(
    id: &str,
    node_type: &str,
    kind: NodeKind,
    ports: [(&str, PortDirection); N],
) -> NodeDescriptor {
    let node = node_id(id);
    NodeDescriptor::new(
        node.clone(),
        NodeTypeName::new(node_type).expect("static type"),
        kind,
        ports.map(|(name, direction)| {
            PortDescriptor::new(node.clone(), port(name), direction, FrameType::Text)
        }),
        ConfigSchema::empty(),
        LifecycleCapabilities::new(true, true, true, true),
    )
}

fn edge(id: &str, from: &str, output: &str, to: &str, input: &str) -> EdgeDescriptor {
    EdgeDescriptor::new(
        EdgeId::new(id).expect("static edge"),
        node_id(from),
        port(output),
        node_id(to),
        port(input),
        FrameType::Text,
        QueuePolicy::default(),
        ValidationPolicy::TypeGateOnly,
        TransformPolicy::Identity,
        EnabledCondition::Always,
        VisibilityDescriptor::default(),
    )
}

fn node_id(value: &str) -> NodeId {
    NodeId::new(value).expect("static node")
}
fn port(value: &str) -> PortName {
    PortName::new(value).expect("static port")
}

fn call_simple(
    callback: extern "C" fn(*mut c_void, *mut ErrorOutput) -> i32,
    user_data: *mut c_void,
) -> voxa_types::Result<()> {
    let mut output = empty_error();
    let status = callback(user_data, &mut output);
    if status == abi::OK {
        Ok(())
    } else {
        Err(callback_error(status, &output))
    }
}

fn callback_error(status: i32, output: &ErrorOutput) -> VoxaError {
    let (code, message) = if status == abi::FOREIGN_EXCEPTION {
        (
            "VOXA-FFI-CPP-EXCEPTION",
            "C++ exception caught by node trampoline",
        )
    } else {
        let code = c_string(&output.code)
            .filter(|value| value.starts_with("VOXA-"))
            .unwrap_or("VOXA-FFI-CALLBACK");
        let message =
            c_string(&output.message).unwrap_or("foreign node callback returned an error");
        return foreign_error(status, code, message);
    };
    foreign_error(status, code, message)
}

fn foreign_error(_status: i32, code: &str, message: &str) -> VoxaError {
    VoxaError::try_new(ErrorCategory::External, code.to_owned(), message.to_owned()).unwrap_or_else(
        |_| {
            VoxaError::new(
                ErrorCategory::External,
                "VOXA-FFI-CALLBACK",
                "foreign callback failure",
            )
        },
    )
}

fn to_voxa_error(error: FfiError) -> VoxaError {
    foreign_error(error.status, error.code, error.message)
}

fn empty_error() -> ErrorOutput {
    ErrorOutput {
        abi_version: abi::ABI_VERSION,
        struct_size: u32::try_from(mem::size_of::<ErrorOutput>()).unwrap_or(u32::MAX),
        status: 0,
        category: 0,
        code: [0; 64],
        message: [0; 256],
    }
}

fn c_string(value: &[i8]) -> Option<&str> {
    let length = value
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(value.len());
    // SAFETY: i8/u8 have identical layout and the slice retains the same bounds.
    let bytes = unsafe { std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), length) };
    std::str::from_utf8(bytes).ok()
}

fn str_view(value: &str) -> StrView {
    StrView {
        data: value.as_ptr().cast(),
        len: value.len(),
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::c_void, mem, ptr, sync::Arc};

    use super::*;
    use crate::abi::{FrameView, Status};

    extern "C" fn process(
        _data: *mut c_void,
        _input: *const FrameView,
        _output: *mut FrameView,
        _error: *mut ErrorOutput,
    ) -> Status {
        abi::OK
    }

    #[test]
    fn close_rejects_late_admission_and_waits_for_active_run() {
        let table = NodeVtable {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<NodeVtable>()).unwrap(),
            user_data: ptr::null_mut(),
            on_prepare: None,
            on_process: Some(process),
            on_signal: None,
            on_finish: None,
            on_abort: None,
            destroy: None,
            capabilities: 0,
            reserved: [0; 3],
        };
        let node = Arc::new(NodeRecord::new(table));
        let guard = node.begin_run().unwrap();
        assert_eq!(node.close_if_idle().unwrap_err().status, abi::BUSY);
        drop(guard);
        node.close_if_idle().unwrap();
        assert_eq!(
            node.begin_run().err().expect("late run rejected").status,
            abi::CLOSED
        );
    }
}
