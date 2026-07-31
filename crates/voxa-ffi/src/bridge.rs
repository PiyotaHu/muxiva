use std::{
    collections::BTreeMap,
    ffi::c_void,
    mem,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use voxa_core::{
    AbortReason, ConfigSchema, EdgeDescriptor, EnabledCondition, GraphBuilder, GraphRunner,
    LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeInstances, NodeKind,
    NodeTypeName, PortDescriptor, PortDirection, PortName, QueuePolicy, TransformPolicy,
    ValidationPolicy, VisibilityDescriptor,
};
use voxa_types::{EdgeId, ErrorCategory, Frame, FrameType, NodeId, SignalFrame, VoxaError};

use crate::{
    abi::{self, AbortReasonView, ErrorOutput, NodeVtable, StrView},
    error::FfiError,
    frame::{borrowed_text_view, copy_frame},
};

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
