#![allow(unsafe_code)]
#![forbid(unsafe_op_in_unsafe_fn)]
//! Audited, copy-owned C ABI v1 and C++ node bridge for Voxa.

mod abi;
mod bridge;
mod error;
mod frame;
mod handles;

use std::{
    mem,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr,
};

use abi::{ErrorOutput, FrameView, NodeVtable, Status, Token};
use error::{boundary, FfiError};
use handles::{Entry, Kind};

pub use abi::{ABI_VERSION as VOXA_ABI_VERSION_V1, MAX_COPY_BYTES};

#[no_mangle]
pub extern "C" fn voxa_abi_version_v1() -> u32 {
    catch_unwind(AssertUnwindSafe(|| abi::ABI_VERSION)).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn voxa_capabilities_v1() -> u64 {
    catch_unwind(AssertUnwindSafe(|| 1_u64 << 0)).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn voxa_runtime_create_v1(out: *mut Token, error: *mut ErrorOutput) -> Status {
    boundary(error, || {
        require_output(out)?;
        let token = handles::insert(Entry::Runtime);
        let _ = abi::write_value(out, token);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_runtime_release_v1(runtime: Token) -> Status {
    release_boundary(|| handles::release(runtime, Kind::Runtime).map(drop))
}

#[no_mangle]
pub extern "C" fn voxa_session_create_v1(
    runtime: Token,
    out: *mut Token,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        handles::contains(runtime, Kind::Runtime)?;
        require_output(out)?;
        let token = handles::insert(Entry::Session);
        let _ = abi::write_value(out, token);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_session_release_v1(session: Token) -> Status {
    release_boundary(|| handles::release(session, Kind::Session).map(drop))
}

#[no_mangle]
pub extern "C" fn voxa_frame_copy_v1(
    frame: *const FrameView,
    out: *mut Token,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        require_output(out)?;
        let owned = frame::copy_frame(frame)?;
        let _copied_payload_len = owned.copied_payload_len();
        let token = handles::insert(Entry::Frame(owned));
        let _ = abi::write_value(out, token);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_frame_release_v1(frame: Token) -> Status {
    release_boundary(|| {
        match handles::release(frame, Kind::Frame)? {
            Entry::Frame(owned) => drop(owned),
            _ => {
                return Err(FfiError::internal(
                    "VOXA-FFI-REGISTRY",
                    "frame registry kind changed",
                ))
            }
        }
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_frame_retain_v1(_frame: Token) -> Status {
    catch_unwind(AssertUnwindSafe(|| abi::UNSUPPORTED)).unwrap_or(abi::PANIC)
}

#[no_mangle]
pub extern "C" fn voxa_node_create_v1(
    vtable: *const NodeVtable,
    out: *mut Token,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        require_output(out)?;
        if !abi::aligned(vtable) {
            return Err(FfiError::validation(
                "VOXA-FFI-VTABLE",
                "node vtable is null or unaligned",
            ));
        }
        let prefix = abi::read_copy(vtable.cast::<abi::AbiPrefix>()).ok_or_else(|| {
            FfiError::validation("VOXA-FFI-VTABLE", "node vtable is null or unaligned")
        })?;
        let expected = u32::try_from(mem::size_of::<NodeVtable>()).unwrap_or(u32::MAX);
        if prefix.abi_version != abi::ABI_VERSION || prefix.struct_size != expected {
            return Err(FfiError::abi(
                "node vtable version or size does not match v1",
            ));
        }
        let vtable = abi::read_copy(vtable).ok_or_else(|| {
            FfiError::validation("VOXA-FFI-VTABLE", "node vtable is not readable")
        })?;
        if vtable.reserved != [0; 3] {
            return Err(FfiError::validation(
                "VOXA-FFI-VTABLE",
                "node vtable reserved fields must be zero",
            ));
        }
        if vtable.on_process.is_none() {
            return Err(FfiError::validation(
                "VOXA-FFI-VTABLE",
                "node vtable requires on_process",
            ));
        }
        let token = handles::insert(Entry::Node(std::sync::Arc::new(bridge::NodeRecord::new(
            vtable,
        ))));
        let _ = abi::write_value(out, token);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_node_release_v1(node: Token) -> Status {
    release_boundary(|| {
        let record = handles::node(node)?;
        record.close_if_idle()?;
        match handles::release(node, Kind::Node)? {
            Entry::Node(record) => drop(record),
            _ => {
                return Err(FfiError::internal(
                    "VOXA-FFI-REGISTRY",
                    "node registry kind changed",
                ))
            }
        }
        Ok(())
    })
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn voxa_runtime_run_text_v1(
    runtime: Token,
    node: Token,
    input: *const FrameView,
    output: *mut i8,
    output_capacity: usize,
    output_len: *mut usize,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        handles::contains(runtime, Kind::Runtime)?;
        let record = handles::node(node)?;
        require_output(output_len)?;
        if output_capacity != 0 && output.is_null() {
            return Err(FfiError::validation(
                "VOXA-FFI-OUTPUT",
                "nonzero output capacity requires a buffer",
            ));
        }
        let input = frame::copy_frame(input)?.to_rust_text()?;
        let result = bridge::run_text_graph(record, input)?;
        let required = result
            .len()
            .checked_add(1)
            .ok_or_else(|| FfiError::validation("VOXA-FFI-OUTPUT", "output length overflow"))?;
        let _ = abi::write_value(output_len, result.len());
        if output_capacity < required {
            return Err(FfiError::validation(
                "VOXA-FFI-OUTPUT",
                "output buffer is too small",
            ));
        }
        // SAFETY: buffer was checked non-null and caller declares output_capacity writable bytes.
        unsafe {
            ptr::copy_nonoverlapping(result.as_ptr(), output.cast::<u8>(), result.len());
            *output.add(result.len()) = 0;
        }
        Ok(())
    })
}

fn require_output<T>(pointer: *mut T) -> Result<(), FfiError> {
    if abi::aligned(pointer.cast_const()) {
        Ok(())
    } else {
        Err(FfiError::validation(
            "VOXA-FFI-OUTPUT",
            "mandatory output pointer is null or unaligned",
        ))
    }
}

fn release_boundary(operation: impl FnOnce() -> Result<(), FfiError>) -> Status {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => abi::OK,
        Ok(Err(error)) => error.status,
        Err(_) => abi::PANIC,
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;

    use super::*;

    fn error_output() -> ErrorOutput {
        ErrorOutput {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<ErrorOutput>()).unwrap(),
            status: 0,
            category: 0,
            code: [0; 64],
            message: [0; 256],
        }
    }

    extern "C" fn process(
        _data: *mut c_void,
        _input: *const FrameView,
        _output: *mut FrameView,
        _error: *mut ErrorOutput,
    ) -> Status {
        abi::OK
    }

    #[test]
    fn panic_boundary_maps_without_crossing() {
        let status = boundary(ptr::null_mut(), || -> Result<(), FfiError> {
            panic!("contained")
        });
        assert_eq!(status, abi::PANIC);
    }

    #[test]
    fn token_layout_is_stable() {
        assert_eq!(mem::size_of::<Token>(), 16);
        assert_eq!(mem::align_of::<Token>(), 8);
    }

    #[test]
    fn repeated_release_is_a_stable_closed_failure() {
        let mut token = Token::INVALID;
        let mut error = error_output();
        assert_eq!(voxa_runtime_create_v1(&mut token, &mut error), abi::OK);
        assert_eq!(voxa_runtime_release_v1(token), abi::OK);
        assert_eq!(voxa_runtime_release_v1(token), abi::CLOSED);
    }

    #[test]
    fn node_vtable_rejects_wrong_abi_before_registration() {
        let table = NodeVtable {
            abi_version: abi::ABI_VERSION + 1,
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
        let mut token = Token::INVALID;
        let mut error = error_output();
        assert_eq!(
            voxa_node_create_v1(&table, &mut token, &mut error),
            abi::ABI_MISMATCH
        );
        assert_eq!(error.status, abi::ABI_MISMATCH);
    }

    #[test]
    fn copied_text_does_not_borrow_the_foreign_buffer() {
        fn string_view(value: &str) -> abi::StrView {
            abi::StrView {
                data: value.as_ptr().cast(),
                len: value.len(),
            }
        }
        let frame_id = String::from("copy-frame");
        let clock = String::from("copy.clock");
        let stream = String::from("copy-stream");
        let trace = String::from("copy-trace");
        let mut text = String::from("original");
        let mut view = abi::empty_frame_view();
        view.header.frame_type = 3;
        view.header.clock_kind = 2;
        view.header.frame_id = string_view(&frame_id);
        view.header.clock_domain_id = string_view(&clock);
        view.header.stream_id = string_view(&stream);
        view.header.trace_id = string_view(&trace);
        view.payload = abi::FramePayload {
            text: abi::TextPayload {
                text: string_view(&text),
                media_type: abi::StrView {
                    data: ptr::null(),
                    len: 0,
                },
                reserved: [0; 2],
            },
        };
        let owned = frame::copy_frame(&view).unwrap();
        text.replace_range(.., "xxxxxxxx");
        match owned.payload {
            frame::OwnedPayload::Text(value) => assert_eq!(value, "original"),
            _ => panic!("text payload expected"),
        }
    }
}
