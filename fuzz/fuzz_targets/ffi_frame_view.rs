#![no_main]

use libfuzzer_sys::fuzz_target;
use std::{ffi::c_void, mem};

#[repr(C)]
#[derive(Clone, Copy)]
struct Token {
    slot: u64,
    generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct StrView {
    data: *const i8,
    len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FrameHeader {
    abi_version: u32,
    struct_size: u32,
    frame_type: u32,
    clock_kind: u32,
    timestamp_ns: i64,
    sequence_id: u64,
    frame_id: StrView,
    clock_domain_id: StrView,
    stream_id: StrView,
    trace_id: StrView,
    reserved: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TextPayload {
    text: StrView,
    media_type: StrView,
    reserved: [u64; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
union FramePayload {
    text: TextPayload,
    storage: [u64; 12],
}

#[repr(C)]
struct FrameView {
    header: FrameHeader,
    payload: FramePayload,
}

extern "C" {
    fn voxa_frame_copy_v1(frame: *const FrameView, out: *mut Token, error: *mut c_void) -> i32;
    fn voxa_frame_release_v1(frame: Token) -> i32;
}

fuzz_target!(|data: &[u8]| {
    // Force the actual Voxa rlib into the final fuzz binary before resolving C symbols below.
    let _ = voxa_ffi::voxa_abi_version_v1();
    let split = data.len() / 2;
    let (identity, text) = data.split_at(split);
    let view = |bytes: &[u8]| StrView {
        data: bytes.as_ptr().cast(),
        len: bytes.len(),
    };
    let mut output = Token {
        slot: u64::MAX,
        generation: 0,
    };
    let frame = FrameView {
        header: FrameHeader {
            abi_version: 0x0001_0000,
            struct_size: mem::size_of::<FrameHeader>() as u32,
            frame_type: 3,
            clock_kind: 1,
            timestamp_ns: 0,
            sequence_id: 0,
            frame_id: view(identity),
            clock_domain_id: view(b"clock"),
            stream_id: view(b"stream"),
            trace_id: view(b"trace"),
            reserved: [0; 4],
        },
        payload: FramePayload {
            text: TextPayload {
                text: view(text),
                media_type: view(b"text/plain"),
                reserved: [0; 2],
            },
        },
    };
    // All pointers reference owned backing that remains alive for the complete call.
    let status = unsafe { voxa_frame_copy_v1(&frame, &mut output, std::ptr::null_mut()) };
    if status == 0 {
        let _ = unsafe { voxa_frame_release_v1(output) };
    }
});
