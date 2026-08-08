use std::{
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use muxiva_core::NotificationBus;
use muxiva_types::{Frame, NamespacedName};
use pyo3::prelude::*;

use crate::{
    binding_error, domain::PythonNodeExecutionDomain, frame::PyEventFrame,
    subscription::try_enqueue_event,
};

#[pyclass(frozen, name = "Runtime")]
pub struct PyRuntime {
    closed: Arc<AtomicBool>,
    next_session: AtomicU64,
}

#[pymethods]
impl PyRuntime {
    #[new]
    fn new() -> Self {
        Self {
            closed: Arc::new(AtomicBool::new(false)),
            next_session: AtomicU64::new(1),
        }
    }
    fn session(&self) -> PyResult<PySession> {
        if self.closed.load(Ordering::Acquire) {
            return Err(binding_error("MUXIVA-PY-CLOSED", "runtime is closed"));
        }
        Ok(PySession {
            id: format!(
                "python-session-{}",
                self.next_session.fetch_add(1, Ordering::Relaxed)
            ),
            runtime_closed: self.closed.clone(),
            closed: AtomicBool::new(false),
        })
    }
    fn close(&self) -> bool {
        !self.closed.swap(true, Ordering::AcqRel)
    }
    #[getter]
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __exit__(
        &self,
        _ty: &Bound<'_, PyAny>,
        _value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) {
        self.close();
    }
}

#[pyclass(frozen, name = "Session")]
pub struct PySession {
    id: String,
    runtime_closed: Arc<AtomicBool>,
    closed: AtomicBool,
}

#[pymethods]
impl PySession {
    #[getter]
    fn id(&self) -> &str {
        &self.id
    }
    #[getter]
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire) || self.runtime_closed.load(Ordering::Acquire)
    }
    fn close(&self) -> bool {
        !self.closed.swap(true, Ordering::AcqRel)
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __exit__(
        &self,
        _ty: &Bound<'_, PyAny>,
        _value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) {
        self.close();
    }
}

#[pyclass(name = "NotificationBus")]
pub struct PyNotificationBus {
    pub(crate) inner: NotificationBus,
    closed: AtomicBool,
}

#[pymethods]
impl PyNotificationBus {
    #[new]
    #[pyo3(signature = (capacity=64))]
    fn new(capacity: usize) -> PyResult<Self> {
        let capacity = NonZeroUsize::new(capacity)
            .ok_or_else(|| binding_error("MUXIVA-PY-CAPACITY", "capacity must be non-zero"))?;
        Ok(Self {
            inner: NotificationBus::new(capacity),
            closed: AtomicBool::new(false),
        })
    }
    fn publish(&self, event: PyRef<'_, PyEventFrame>) -> PyResult<(usize, usize, usize)> {
        let Frame::Event(event) = event.inner.clone() else {
            unreachable!("typed wrapper invariant")
        };
        let report = self
            .inner
            .publish(event)
            .map_err(|e| binding_error("MUXIVA-PY-NOTIFICATION-BUS", e.to_string()))?;
        Ok((report.matched, report.enqueued, report.dropped_full))
    }
    fn subscribe(
        &self,
        topic: String,
        domain: PyRef<'_, PythonNodeExecutionDomain>,
    ) -> PyResult<u64> {
        let topic = NamespacedName::new(topic)
            .map_err(|e| binding_error("MUXIVA-PY-EVENT-TOPIC", e.to_string()))?;
        let driver = domain.driver.clone();
        let sequence = domain.sequence.clone();
        let timeout = domain.call_timeout;
        let subscription = self
            .inner
            .subscribe(topic, move |event| {
                let _ = try_enqueue_event(&driver, &sequence, event, timeout);
                Ok(())
            })
            .map_err(|e| binding_error("MUXIVA-PY-NOTIFICATION-BUS", e.to_string()))?;
        Ok(subscription.get())
    }
    fn close(&self) -> bool {
        if self.closed.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.inner.stop(Duration::from_secs(1));
        true
    }
    #[getter]
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __exit__(
        &self,
        _ty: &Bound<'_, PyAny>,
        _value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) {
        self.close();
    }
}
