use std::{
    mem,
    panic::{catch_unwind, AssertUnwindSafe},
};

use crate::abi::{self, ErrorOutput, Status};

#[derive(Clone, Copy, Debug)]
pub struct FfiError {
    pub status: Status,
    pub category: i32,
    pub code: &'static str,
    pub message: &'static str,
}

impl FfiError {
    pub const fn validation(code: &'static str, message: &'static str) -> Self {
        Self {
            status: abi::INVALID_ARGUMENT,
            category: 1,
            code,
            message,
        }
    }

    pub const fn abi(message: &'static str) -> Self {
        Self {
            status: abi::ABI_MISMATCH,
            category: 1,
            code: "MUXIVA-FFI-ABI",
            message,
        }
    }

    pub const fn handle(status: Status, message: &'static str) -> Self {
        Self {
            status,
            category: 2,
            code: "MUXIVA-FFI-HANDLE",
            message,
        }
    }

    pub const fn internal(code: &'static str, message: &'static str) -> Self {
        Self {
            status: abi::INTERNAL,
            category: 6,
            code,
            message,
        }
    }
}

pub fn boundary(
    out_error: *mut ErrorOutput,
    operation: impl FnOnce() -> Result<(), FfiError>,
) -> Status {
    if let Err(status) = prepare(out_error) {
        return status;
    }
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => abi::OK,
        Ok(Err(error)) => {
            write_error(out_error, error);
            error.status
        }
        Err(_) => {
            let error = FfiError {
                status: abi::PANIC,
                category: 6,
                code: "MUXIVA-FFI-RUST-PANIC",
                message: "Rust panic caught at the C ABI boundary",
            };
            write_error(out_error, error);
            abi::PANIC
        }
    }
}

fn prepare(out_error: *mut ErrorOutput) -> Result<(), Status> {
    if out_error.is_null() {
        return Ok(());
    }
    let Some(prefix) = abi::read_copy(out_error.cast_const().cast::<abi::AbiPrefix>()) else {
        return Err(abi::INVALID_ARGUMENT);
    };
    let full_size = u32::try_from(mem::size_of::<ErrorOutput>()).unwrap_or(u32::MAX);
    if prefix.abi_version != abi::ABI_VERSION || prefix.struct_size < full_size {
        return Err(abi::ABI_MISMATCH);
    }
    let empty = ErrorOutput {
        abi_version: abi::ABI_VERSION,
        struct_size: full_size,
        status: abi::OK,
        category: 0,
        code: [0; 64],
        message: [0; 256],
    };
    if abi::write_value(out_error, empty) {
        Ok(())
    } else {
        Err(abi::INVALID_ARGUMENT)
    }
}

pub fn write_error(out_error: *mut ErrorOutput, error: FfiError) {
    if out_error.is_null() {
        return;
    }
    let Some(mut output) = abi::read_copy(out_error.cast_const()) else {
        return;
    };
    output.status = error.status;
    output.category = error.category;
    copy_c_string(&mut output.code, error.code);
    copy_c_string(&mut output.message, error.message);
    let _ = abi::write_value(out_error, output);
}

fn copy_c_string<const N: usize>(output: &mut [i8; N], input: &str) {
    *output = [0; N];
    for (destination, source) in output
        .iter_mut()
        .take(N.saturating_sub(1))
        .zip(input.bytes())
    {
        *destination = source as i8;
    }
}
