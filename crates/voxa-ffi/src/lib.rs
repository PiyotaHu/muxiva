#![allow(unsafe_code)]
#![forbid(unsafe_op_in_unsafe_fn)]
//! Audited, copy-owned C ABI v1 and C++ node bridge for Voxa.

mod abi;
mod bridge;
mod error;
mod frame;
mod handles;
mod ingress;

use std::{
    mem,
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    ptr,
    sync::Arc,
};

use abi::{
    ErrorOutput, FrameView, GraphRunSummary, MultimodalNodeFactoryView, NodeFactoryView,
    NodeVtable, Status, Token,
};
use error::{boundary, FfiError};
use handles::{Entry, Kind};
use serde::Deserialize;

pub use abi::{ABI_VERSION as VOXA_ABI_VERSION_V1, MAX_COPY_BYTES};

/// Loads one trusted, in-process C++ multimodal Node Pack through ABI v1.
///
/// The returned registration owns the dynamic library for at least as long as
/// any factory or Node instance can call into it.
pub fn load_cpp_multimodal_node_pack(path: &Path) -> Result<voxa_core::NodeRegistration, String> {
    type Entrypoint = unsafe extern "C" fn() -> abi::MultimodalNodeFactoryView;

    // SAFETY: loading native code is explicitly restricted to a user-installed
    // Node Pack path. ABI fields and every callback are validated before the
    // registration becomes visible to the Runtime.
    let library = Arc::new(
        unsafe { libloading::Library::new(path) }
            .map_err(|error| format!("cannot load C++ Node Pack `{}`: {error}", path.display()))?,
    );
    // SAFETY: the symbol name and return layout are the public Voxa ABI v1
    // contract. cpp_multimodal_factory_spec validates the returned descriptor.
    let view = unsafe {
        let entrypoint: libloading::Symbol<'_, Entrypoint> = library
            .get(b"voxa_node_pack_factory\0")
            .map_err(|error| format!("C++ Node Pack has no v1 factory symbol: {error}"))?;
        entrypoint()
    };
    let spec = cpp_multimodal_factory_spec(&view)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    Ok(bridge::cpp_multimodal_registration(spec, Some(library)))
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IngressConfig {
    pub abi_version: u32,
    pub struct_size: u32,
    pub item_capacity: usize,
    pub byte_capacity: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IngressStats {
    pub abi_version: u32,
    pub struct_size: u32,
    pub accepted: u64,
    pub full_drops: u64,
    pub closed_drops: u64,
    pub queued_items: usize,
    pub queued_bytes: usize,
}

#[no_mangle]
pub extern "C" fn voxa_abi_version_v1() -> u32 {
    catch_unwind(AssertUnwindSafe(|| abi::ABI_VERSION)).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn voxa_capabilities_v1() -> u64 {
    catch_unwind(AssertUnwindSafe(|| (1_u64 << 0) | (1_u64 << 2))).unwrap_or(0)
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
pub extern "C" fn voxa_session_ingress_create_v1(
    session: Token,
    config: *const IngressConfig,
    out: *mut Token,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        handles::contains(session, Kind::Session)?;
        require_output(out)?;
        let config = abi::read_copy(config).ok_or_else(|| {
            FfiError::validation("VOXA-FFI-INGRESS", "ingress config is null or unaligned")
        })?;
        let expected = u32::try_from(mem::size_of::<IngressConfig>()).unwrap_or(u32::MAX);
        if config.abi_version != abi::ABI_VERSION || config.struct_size != expected {
            return Err(FfiError::abi(
                "ingress config version or size does not match v1",
            ));
        }
        if config.item_capacity == 0
            || config.byte_capacity == 0
            || config.byte_capacity > abi::MAX_COPY_BYTES
        {
            return Err(FfiError::validation(
                "VOXA-FFI-INGRESS",
                "ingress bounds are invalid",
            ));
        }
        let token = handles::insert(Entry::Ingress(std::sync::Arc::new(
            ingress::ExternalIngress::new(config.item_capacity, config.byte_capacity),
        )));
        let _ = abi::write_value(out, token);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_session_ingress_clone_v1(
    ingress: Token,
    out: *mut Token,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        require_output(out)?;
        let handle = handles::ingress(ingress)?;
        let _ = abi::write_value(out, handles::insert(Entry::Ingress(handle)));
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_session_ingress_close_v1(ingress: Token) -> Status {
    release_boundary(|| {
        handles::ingress(ingress)?.close();
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_session_ingress_release_v1(ingress: Token) -> Status {
    release_boundary(|| handles::release(ingress, Kind::Ingress).map(drop))
}

#[no_mangle]
pub extern "C" fn voxa_session_ingress_try_submit_v1(
    ingress: Token,
    frame: *const FrameView,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        let ingress = handles::try_ingress(ingress)?;
        let frame = frame::copy_frame(frame)?;
        match ingress.try_submit(frame) {
            Ok(()) => Ok(()),
            Err(ingress::SubmitError::Full) => Err(FfiError::handle(
                abi::QUEUE_FULL,
                "external ingress is full",
            )),
            Err(ingress::SubmitError::Closed) => {
                Err(FfiError::handle(abi::CLOSED, "external ingress is closed"))
            }
        }
    })
}

#[no_mangle]
pub extern "C" fn voxa_session_ingress_stats_v1(
    ingress: Token,
    out: *mut IngressStats,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        require_output(out)?;
        let ingress = handles::ingress(ingress)?;
        let (accepted, full_drops, closed_drops, queued_items, queued_bytes) = ingress.stats();
        let stats = IngressStats {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<IngressStats>()).unwrap_or(u32::MAX),
            accepted,
            full_drops,
            closed_drops,
            queued_items,
            queued_bytes,
        };
        let _ = abi::write_value(out, stats);
        Ok(())
    })
}

#[no_mangle]
pub extern "C" fn voxa_session_ingress_try_pop_v1(
    ingress: Token,
    out: *mut Token,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        require_output(out)?;
        let ingress = handles::ingress(ingress)?;
        let frame = ingress
            .pop()
            .ok_or_else(|| FfiError::handle(abi::BUSY, "external ingress is empty"))?;
        let _ = abi::write_value(out, handles::insert(Entry::Frame(frame)));
        Ok(())
    })
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

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn voxa_runtime_run_graph_v1(
    runtime: Token,
    graph_json: abi::StrView,
    factories: *const NodeFactoryView,
    factory_count: usize,
    timeout_ms: u64,
    summary: *mut GraphRunSummary,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        handles::contains(runtime, Kind::Runtime)?;
        require_output(summary)?;
        if timeout_ms == 0 || timeout_ms > 60 * 60 * 1_000 {
            return Err(FfiError::validation(
                "VOXA-FFI-GRAPH-TIMEOUT",
                "Graph timeout must be between 1 and 3600000 milliseconds",
            ));
        }
        if factory_count > 4_096 || (factory_count != 0 && !abi::aligned(factories)) {
            return Err(FfiError::validation(
                "VOXA-FFI-GRAPH-FACTORIES",
                "Graph factory array is null or exceeds the hard limit",
            ));
        }
        let graph_json = abi::copy_utf8(graph_json).map_err(|_| {
            FfiError::validation("VOXA-FFI-GRAPH-JSON", "Graph JSON is not valid UTF-8")
        })?;
        let views = if factory_count == 0 {
            &[]
        } else {
            // SAFETY: the caller declares `factory_count` readable entries for this call.
            unsafe { std::slice::from_raw_parts(factories, factory_count) }
        };
        let specs = views
            .iter()
            .map(cpp_factory_spec)
            .collect::<Result<Vec<_>, _>>()?;
        let worker_total = bridge::run_registered_graph(
            &graph_json,
            &specs,
            std::time::Duration::from_millis(timeout_ms),
        )?;
        let worker_total = u32::try_from(worker_total).map_err(|_| {
            FfiError::internal("VOXA-FFI-GRAPH-SUMMARY", "worker count exceeds u32")
        })?;
        let value = GraphRunSummary {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<GraphRunSummary>()).unwrap_or(u32::MAX),
            worker_total,
            reserved: [0; 4],
        };
        let _ = abi::write_value(summary, value);
        Ok(())
    })
}

#[no_mangle]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub extern "C" fn voxa_runtime_run_multimodal_graph_v1(
    runtime: Token,
    graph_json: abi::StrView,
    factories: *const MultimodalNodeFactoryView,
    factory_count: usize,
    timeout_ms: u64,
    summary: *mut GraphRunSummary,
    error: *mut ErrorOutput,
) -> Status {
    boundary(error, || {
        handles::contains(runtime, Kind::Runtime)?;
        require_output(summary)?;
        if timeout_ms == 0 || timeout_ms > 60 * 60 * 1_000 {
            return Err(FfiError::validation(
                "VOXA-FFI-GRAPH-TIMEOUT",
                "Graph timeout must be between 1 and 3600000 milliseconds",
            ));
        }
        if factory_count > 4_096 || (factory_count != 0 && !abi::aligned(factories)) {
            return Err(FfiError::validation(
                "VOXA-FFI-GRAPH-FACTORIES",
                "Graph factory array is null or exceeds the hard limit",
            ));
        }
        let graph_json = abi::copy_utf8(graph_json).map_err(|_| {
            FfiError::validation("VOXA-FFI-GRAPH-JSON", "Graph JSON is not valid UTF-8")
        })?;
        let views = if factory_count == 0 {
            &[]
        } else {
            // SAFETY: caller declares factory_count readable descriptors for this call.
            unsafe { std::slice::from_raw_parts(factories, factory_count) }
        };
        let specs = views
            .iter()
            .map(cpp_multimodal_factory_spec)
            .collect::<Result<Vec<_>, _>>()?;
        let worker_total = bridge::run_registered_multimodal_graph(
            &graph_json,
            &specs,
            std::time::Duration::from_millis(timeout_ms),
        )?;
        let value = GraphRunSummary {
            abi_version: abi::ABI_VERSION,
            struct_size: u32::try_from(mem::size_of::<GraphRunSummary>()).unwrap_or(u32::MAX),
            worker_total: u32::try_from(worker_total).map_err(|_| {
                FfiError::internal("VOXA-FFI-GRAPH-SUMMARY", "worker count exceeds u32")
            })?,
            reserved: [0; 4],
        };
        let _ = abi::write_value(summary, value);
        Ok(())
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CppPortDocument {
    name: String,
    direction: String,
    frame_type: String,
}

fn cpp_multimodal_factory_spec(
    view: &MultimodalNodeFactoryView,
) -> Result<bridge::CppMultimodalFactorySpec, FfiError> {
    let expected = u32::try_from(mem::size_of::<MultimodalNodeFactoryView>()).unwrap_or(u32::MAX);
    if view.abi_version != abi::ABI_VERSION || view.struct_size != expected {
        return Err(FfiError::abi(
            "multimodal Graph Node factory version or size does not match v1",
        ));
    }
    if view.reserved0 != 0 || view.reserved != [0; 4] {
        return Err(FfiError::validation(
            "VOXA-FFI-GRAPH-FACTORY",
            "reserved fields must be zero",
        ));
    }
    let required = |value| {
        abi::copy_str(value, true).map_err(|_| {
            FfiError::validation(
                "VOXA-FFI-GRAPH-FACTORY",
                "factory contains an invalid string",
            )
        })
    };
    let node_type = voxa_core::NodeTypeName::new(required(view.node_type)?)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "invalid node type"))?;
    let version = voxa_core::NodeFactoryVersion::new(required(view.version)?)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "invalid version"))?;
    let kind = match view.kind {
        1 => voxa_core::NodeKind::Source,
        2 => voxa_core::NodeKind::Transform,
        3 => voxa_core::NodeKind::Sink,
        _ => {
            return Err(FfiError::validation(
                "VOXA-FFI-GRAPH-FACTORY",
                "invalid node kind",
            ))
        }
    };
    let ports_encoded = abi::copy_utf8(view.ports_json).map_err(|_| {
        FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "ports JSON is invalid UTF-8")
    })?;
    let ports = serde_json::from_str::<Vec<CppPortDocument>>(&ports_encoded)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "ports JSON is invalid"))?
        .into_iter()
        .map(|port| {
            let name = voxa_core::PortName::new(port.name)
                .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "invalid port name"))?;
            let direction = match port.direction.as_str() {
                "input" => voxa_core::PortDirection::Input,
                "output" => voxa_core::PortDirection::Output,
                _ => {
                    return Err(FfiError::validation(
                        "VOXA-FFI-GRAPH-FACTORY",
                        "invalid port direction",
                    ))
                }
            };
            let frame_type = match port.frame_type.as_str() {
                "audio" => voxa_types::FrameType::Audio,
                "video" => voxa_types::FrameType::Video,
                "text" => voxa_types::FrameType::Text,
                "byte" => voxa_types::FrameType::Byte,
                "signal" => voxa_types::FrameType::Signal,
                "event" => voxa_types::FrameType::Event,
                _ => {
                    return Err(FfiError::validation(
                        "VOXA-FFI-GRAPH-FACTORY",
                        "invalid port frame type",
                    ))
                }
            };
            Ok((name, direction, frame_type))
        })
        .collect::<Result<Vec<_>, FfiError>>()?;
    let schema_encoded = abi::copy_utf8(view.config_schema_json).map_err(|_| {
        FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "schema JSON is invalid UTF-8")
    })?;
    let schema_json: serde_json::Value = serde_json::from_str(&schema_encoded)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "schema JSON is invalid"))?;
    let config_schema =
        voxa_core::ConfigSchema::new(voxa_graph_json::value_from_json(&schema_json).map_err(
            |_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "schema value is invalid"),
        )?);
    let create = view.create.ok_or_else(|| {
        FfiError::validation(
            "VOXA-FFI-GRAPH-FACTORY",
            "factory requires a create callback",
        )
    })?;
    Ok(bridge::CppMultimodalFactorySpec {
        node_type,
        version,
        kind,
        ports,
        config_schema,
        user_data: view.user_data as usize,
        create,
    })
}

fn cpp_factory_spec(view: &NodeFactoryView) -> Result<bridge::CppFactorySpec, FfiError> {
    let expected = u32::try_from(mem::size_of::<NodeFactoryView>()).unwrap_or(u32::MAX);
    if view.abi_version != abi::ABI_VERSION || view.struct_size != expected {
        return Err(FfiError::abi(
            "Graph Node factory version or size does not match v1",
        ));
    }
    if view.reserved != [0; 4] {
        return Err(FfiError::validation(
            "VOXA-FFI-GRAPH-FACTORY",
            "Graph Node factory reserved fields must be zero",
        ));
    }
    let required = |value| {
        abi::copy_str(value, true).map_err(|_| {
            FfiError::validation(
                "VOXA-FFI-GRAPH-FACTORY",
                "Graph Node factory contains an invalid string",
            )
        })
    };
    let node_type = voxa_core::NodeTypeName::new(required(view.node_type)?)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "invalid node type"))?;
    let version = voxa_core::NodeFactoryVersion::new(required(view.version)?)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "invalid version"))?;
    let input_port = voxa_core::PortName::new(required(view.input_port)?)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "invalid input port"))?;
    let output_port = voxa_core::PortName::new(required(view.output_port)?)
        .map_err(|_| FfiError::validation("VOXA-FFI-GRAPH-FACTORY", "invalid output port"))?;
    let create = view.create.ok_or_else(|| {
        FfiError::validation(
            "VOXA-FFI-GRAPH-FACTORY",
            "Graph Node factory requires a create callback",
        )
    })?;
    Ok(bridge::CppFactorySpec {
        node_type,
        version,
        input_port,
        output_port,
        user_data: view.user_data as usize,
        create,
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

    #[test]
    fn i420_video_is_tightly_validated_and_copied() {
        fn string_view(value: &str) -> abi::StrView {
            abi::StrView {
                data: value.as_ptr().cast(),
                len: value.len(),
            }
        }
        let frame_id = String::from("i420-frame");
        let clock = String::from("media");
        let stream = String::from("camera");
        let trace = String::from("trace");
        let mut pixels = [7_u8; 24]; // 4x4 Y plus 2x2 U and V.
        let mut view = abi::empty_frame_view();
        view.header.frame_type = 2;
        view.header.clock_kind = 2;
        view.header.frame_id = string_view(&frame_id);
        view.header.clock_domain_id = string_view(&clock);
        view.header.stream_id = string_view(&stream);
        view.header.trace_id = string_view(&trace);
        view.payload = abi::FramePayload {
            video: abi::VideoPayload {
                width: 4,
                height: 4,
                pixel_format: 2,
                plane_count: 3,
                bytes: abi::BytesView {
                    data: pixels.as_ptr(),
                    len: pixels.len(),
                },
                reserved: [0; 4],
            },
        };
        let owned = frame::copy_frame(&view).unwrap();
        pixels.fill(9);
        match &owned.payload {
            frame::OwnedPayload::Video { bytes, .. } => assert_eq!(bytes, &[7_u8; 24]),
            _ => panic!("video payload expected"),
        }
        let rust = owned.to_rust().unwrap();
        assert_eq!(
            rust.as_video().unwrap().data().pixel_format(),
            voxa_types::PixelFormat::Yuv420p
        );

        // The ABI is intentionally tight; a missing chroma byte is rejected.
        view.payload.video.bytes.len = 23;
        assert!(frame::copy_frame(&view).is_err());
    }
}
