use std::{ffi::c_void, mem, ptr};

pub const ABI_VERSION: u32 = 0x0001_0000;
pub const MAX_COPY_BYTES: usize = 16 * 1024 * 1024;

pub type Status = i32;
pub const OK: Status = 0;
pub const INVALID_ARGUMENT: Status = 1;
pub const ABI_MISMATCH: Status = 2;
pub const INVALID_HANDLE: Status = 3;
pub const CLOSED: Status = 4;
pub const BUSY: Status = 5;
#[allow(dead_code)]
pub const QUEUE_FULL: Status = 6;
#[allow(dead_code)]
pub const CANCELLED: Status = 7;
#[allow(dead_code)]
pub const TIMEOUT: Status = 8;
pub const UNSUPPORTED: Status = 9;
pub const EXTERNAL: Status = 10;
pub const FOREIGN_EXCEPTION: Status = 11;
pub const INTERNAL: Status = 12;
pub const PANIC: Status = 13;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Token {
    pub slot: u64,
    pub generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AbiPrefix {
    pub abi_version: u32,
    pub struct_size: u32,
}

impl Token {
    pub const INVALID: Self = Self {
        slot: u64::MAX,
        generation: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StrView {
    pub data: *const i8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BytesView {
    pub data: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ErrorOutput {
    pub abi_version: u32,
    pub struct_size: u32,
    pub status: Status,
    pub category: i32,
    pub code: [i8; 64],
    pub message: [i8; 256],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FrameHeader {
    pub abi_version: u32,
    pub struct_size: u32,
    pub frame_type: u32,
    pub clock_kind: u32,
    pub timestamp_ns: i64,
    pub sequence_id: u64,
    pub frame_id: StrView,
    pub clock_domain_id: StrView,
    pub stream_id: StrView,
    pub trace_id: StrView,
    pub reserved: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioPayload {
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub sample_format: u16,
    pub layout: u32,
    pub reserved0: u32,
    pub samples_per_channel: u64,
    pub bytes: BytesView,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VideoPayload {
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub plane_count: u32,
    pub bytes: BytesView,
    pub reserved: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TextPayload {
    pub text: StrView,
    pub media_type: StrView,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BytePayload {
    pub bytes: BytesView,
    pub media_type: StrView,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SignalPayload {
    pub signal_name: StrView,
    pub source_node_id: StrView,
    pub value: BytesView,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EventPayload {
    pub topic: StrView,
    pub value: BytesView,
    pub reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union FramePayload {
    pub audio: AudioPayload,
    pub video: VideoPayload,
    pub text: TextPayload,
    pub bytes: BytePayload,
    pub signal: SignalPayload,
    pub event: EventPayload,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FrameView {
    pub header: FrameHeader,
    pub payload: FramePayload,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AbortReasonView {
    pub abi_version: u32,
    pub struct_size: u32,
    pub category: i32,
    pub stage: i32,
    pub code: StrView,
    pub message: StrView,
}

pub type SimpleCallback = extern "C" fn(*mut c_void, *mut ErrorOutput) -> Status;
pub type ProcessCallback =
    extern "C" fn(*mut c_void, *const FrameView, *mut FrameView, *mut ErrorOutput) -> Status;
pub type SignalCallback = extern "C" fn(*mut c_void, *const FrameView, *mut ErrorOutput) -> Status;
pub type AbortCallback = extern "C" fn(*mut c_void, *const AbortReasonView);
pub type DestroyCallback = extern "C" fn(*mut c_void);
pub type FactoryCreateCallback =
    extern "C" fn(*mut c_void, StrView, *mut NodeVtable, *mut ErrorOutput) -> Status;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NamedFrameView {
    pub output_port: StrView,
    pub frame: FrameView,
}

pub type OwnedPayloadReleaseCallback = extern "C" fn(*mut c_void);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OwnedNamedFrameView {
    pub output_port: StrView,
    pub frame: FrameView,
    pub payload_owner: *mut c_void,
    pub release_payload: Option<OwnedPayloadReleaseCallback>,
    pub reserved: [u64; 2],
}

pub const NODE_METRIC_COUNTER_ADD: u32 = 1;
pub const NODE_METRIC_GAUGE_SET: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NodeMetricView {
    pub name: StrView,
    pub operation: u32,
    pub reserved0: u32,
    pub value: u64,
}

pub type GraphProcessCallback = extern "C" fn(
    *mut c_void,
    *const FrameView,
    StrView,
    *mut *const NamedFrameView,
    *mut usize,
    *mut ErrorOutput,
) -> Status;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GraphNodeVtable {
    pub abi_version: u32,
    pub struct_size: u32,
    pub user_data: *mut c_void,
    pub on_prepare: Option<SimpleCallback>,
    pub on_process: Option<GraphProcessCallback>,
    pub on_signal: Option<SignalCallback>,
    pub on_finish: Option<SimpleCallback>,
    pub on_abort: Option<AbortCallback>,
    pub destroy: Option<DestroyCallback>,
    pub capabilities: u64,
    pub reserved: [u64; 3],
    pub take_next_source_tick_ns: Option<extern "C" fn(*mut c_void) -> u64>,
    pub take_metrics: Option<extern "C" fn(*mut c_void, *mut *const NodeMetricView, *mut usize)>,
    pub take_owned_emissions:
        Option<extern "C" fn(*mut c_void, *mut *const OwnedNamedFrameView, *mut usize)>,
}

unsafe impl Send for GraphNodeVtable {}
unsafe impl Sync for GraphNodeVtable {}

pub type GraphFactoryCreateCallback =
    extern "C" fn(*mut c_void, StrView, StrView, *mut GraphNodeVtable, *mut ErrorOutput) -> Status;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MultimodalNodeFactoryView {
    pub abi_version: u32,
    pub struct_size: u32,
    pub node_type: StrView,
    pub version: StrView,
    pub kind: u32,
    pub reserved0: u32,
    pub ports_json: StrView,
    pub config_schema_json: StrView,
    pub user_data: *mut c_void,
    pub create: Option<GraphFactoryCreateCallback>,
    pub reserved: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NodeVtable {
    pub abi_version: u32,
    pub struct_size: u32,
    pub user_data: *mut c_void,
    pub on_prepare: Option<SimpleCallback>,
    pub on_process: Option<ProcessCallback>,
    pub on_signal: Option<SignalCallback>,
    pub on_finish: Option<SimpleCallback>,
    pub on_abort: Option<AbortCallback>,
    pub destroy: Option<DestroyCallback>,
    pub capabilities: u64,
    pub reserved: [u64; 3],
}

// The foreign owner guarantees that user_data is usable on Muxiva's serialized node domain
// until destroy. The registry closes admission and drains active calls before destruction.
unsafe impl Send for NodeVtable {}
unsafe impl Sync for NodeVtable {}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NodeFactoryView {
    pub abi_version: u32,
    pub struct_size: u32,
    pub node_type: StrView,
    pub version: StrView,
    pub input_port: StrView,
    pub output_port: StrView,
    pub user_data: *mut c_void,
    pub create: Option<FactoryCreateCallback>,
    pub reserved: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GraphRunSummary {
    pub abi_version: u32,
    pub struct_size: u32,
    pub worker_total: u32,
    pub reserved: [u64; 4],
}

pub fn aligned<T>(pointer: *const T) -> bool {
    !pointer.is_null() && (pointer as usize) % mem::align_of::<T>() == 0
}

pub fn read_copy<T: Copy>(pointer: *const T) -> Option<T> {
    if !aligned(pointer) {
        return None;
    }
    // SAFETY: alignment and non-null were checked; the C contract requires a readable T.
    Some(unsafe { ptr::read(pointer) })
}

pub fn write_value<T>(pointer: *mut T, value: T) -> bool {
    if !aligned(pointer.cast_const()) {
        return false;
    }
    // SAFETY: alignment and non-null were checked; the C contract requires writable storage.
    unsafe { ptr::write(pointer, value) };
    true
}

pub fn copy_bytes(view: BytesView) -> Result<Vec<u8>, &'static str> {
    if view.len > MAX_COPY_BYTES {
        return Err("payload exceeds the v1 copy limit");
    }
    if view.len == 0 {
        return Ok(Vec::new());
    }
    if view.data.is_null() {
        return Err("non-empty byte view has a null pointer");
    }
    // SAFETY: non-null and bounded length were checked; caller keeps the view readable for call.
    Ok(unsafe { std::slice::from_raw_parts(view.data, view.len) }.to_vec())
}

pub fn copy_str(view: StrView, required: bool) -> Result<String, &'static str> {
    let value = copy_utf8(view)?;
    if required && value.is_empty() {
        return Err("required identifier is empty");
    }
    if value.len() > 255 || value.trim() != value || value.bytes().any(|b| b.is_ascii_control()) {
        return Err("identifier violates the stable string contract");
    }
    Ok(value)
}

pub fn copy_utf8(view: StrView) -> Result<String, &'static str> {
    let bytes = copy_bytes(BytesView {
        data: view.data.cast(),
        len: view.len,
    })?;
    String::from_utf8(bytes).map_err(|_| "string view is not UTF-8")
}

pub fn empty_frame_view() -> FrameView {
    // SAFETY: all-zero is a valid initial state for these C POD integer/pointer fields.
    let mut frame = unsafe { mem::zeroed::<FrameView>() };
    frame.header.abi_version = ABI_VERSION;
    frame.header.struct_size = u32::try_from(mem::size_of::<FrameHeader>()).unwrap_or(u32::MAX);
    frame
}

const _: () = assert!(mem::size_of::<Token>() == 16);
