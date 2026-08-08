#![forbid(unsafe_code)]
//! PyO3 boundary for owned Muxiva values and bounded Python node execution.

mod api;
mod domain;
mod frame;
mod graph;
mod isolated;
mod subscription;

use pyo3::{create_exception, exceptions::PyException, prelude::*};

pub use api::{PyNotificationBus, PyRuntime, PySession};
pub use domain::PythonNodeExecutionDomain;
pub use frame::{
    PyAudioFrame, PyByteFrame, PyEventFrame, PyFrame, PySignalFrame, PyTextFrame, PyVideoFrame,
};
pub use graph::PyGraphNodeFactory;

create_exception!(_native, MuxivaError, PyException);

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add("MuxivaError", module.py().get_type::<MuxivaError>())?;
    module.add_class::<PyFrame>()?;
    module.add_class::<PyAudioFrame>()?;
    module.add_class::<PyVideoFrame>()?;
    module.add_class::<PyTextFrame>()?;
    module.add_class::<PyByteFrame>()?;
    module.add_class::<PySignalFrame>()?;
    module.add_class::<PyEventFrame>()?;
    module.add_class::<PyRuntime>()?;
    module.add_class::<PySession>()?;
    module.add_class::<PyNotificationBus>()?;
    module.add_class::<PythonNodeExecutionDomain>()?;
    module.add_class::<PyGraphNodeFactory>()?;
    module.add_function(wrap_pyfunction!(graph::run_graph, module)?)?;
    Ok(())
}

pub(crate) fn binding_error(code: &str, message: impl AsRef<str>) -> PyErr {
    MuxivaError::new_err(format!("{code}: {}", message.as_ref()))
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use pyo3::{prelude::*, types::PyModule};

    #[test]
    fn async_node_runs_on_its_dedicated_loop_and_returns_owned_frame() {
        Python::attach(|py| {
            let module = PyModule::new(py, "_native").unwrap();
            super::_native(&module).unwrap();
            let source = CString::new(
                r#"
import asyncio
class Node:
    def __init__(self): self.loop_ids = []
    async def on_prepare(self):
        self.loop_ids.append(id(asyncio.get_running_loop()))
    async def on_process(self, frame):
        await asyncio.sleep(0.01)
        self.loop_ids.append(id(asyncio.get_running_loop()))
        return frame
node = Node()
"#,
            )
            .unwrap();
            let file = CString::new("domain_test.py").unwrap();
            let name = CString::new("domain_test").unwrap();
            let user = PyModule::from_code(py, &source, &file, &name).unwrap();
            let domain = module
                .getattr("PythonNodeExecutionDomain")
                .unwrap()
                .call1((user.getattr("node").unwrap(),))
                .unwrap();
            domain.call_method0("prepare").unwrap();
            let frame = module
                .getattr("TextFrame")
                .unwrap()
                .call1(("hello",))
                .unwrap();
            let output = domain.call_method1("process", (frame,)).unwrap();
            assert_eq!(output.len().unwrap(), 1);
            domain.call_method0("close").unwrap();
            let ids: Vec<usize> = user
                .getattr("node")
                .unwrap()
                .getattr("loop_ids")
                .unwrap()
                .extract()
                .unwrap();
            assert_eq!(ids.len(), 2);
            assert_eq!(ids[0], ids[1]);
        });
    }

    #[test]
    fn isolated_process_is_rejected_explicitly() {
        Python::attach(|py| {
            let module = PyModule::new(py, "_native").unwrap();
            super::_native(&module).unwrap();
            let object = py.eval(pyo3::ffi::c_str!("object()"), None, None).unwrap();
            let kwargs = pyo3::types::PyDict::new(py);
            kwargs.set_item("isolation", "isolated_process").unwrap();
            let error = module
                .getattr("PythonNodeExecutionDomain")
                .unwrap()
                .call((object,), Some(&kwargs))
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("MUXIVA-PY-ISOLATION-UNSUPPORTED"));
        });
    }
}
