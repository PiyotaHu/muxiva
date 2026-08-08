use pyo3::prelude::*;

use crate::binding_error;

/// V1 deliberately refuses to disguise in-process execution as process isolation.
pub(crate) fn validate_isolation(value: &str) -> PyResult<()> {
    match value {
        "in_process" => Ok(()),
        "isolated_process" => Err(binding_error(
            "MUXIVA-PY-ISOLATION-UNSUPPORTED",
            "isolated_process requires the versioned authenticated IPC worker and is not available in V1",
        )),
        _ => Err(binding_error(
            "MUXIVA-PY-ISOLATION",
            "isolation must be 'in_process' or 'isolated_process'",
        )),
    }
}
