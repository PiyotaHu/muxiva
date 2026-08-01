use std::{
    num::NonZeroUsize,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use pyo3::{
    prelude::*,
    types::{PyList, PyTuple},
};
use voxa_core::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigMap, ForeignCommand,
    ForeignCommandKind, ForeignCompletion, ForeignCompletionKind, ForeignCompletionOutcome,
    ForeignDriverConfig, ForeignNodeDriver, ForeignOrdering, ForeignSubmitOutcome,
};
use voxa_types::Frame;

use crate::{
    binding_error,
    frame::{extract_frame, frame_to_python},
    isolated::validate_isolation,
};

struct DomainState {
    closed: bool,
    terminal_callback_completed: bool,
    thread: Option<thread::JoinHandle<()>>,
    done: Option<mpsc::Receiver<()>>,
}

/// One bounded Python node domain with a dedicated OS thread and asyncio loop.
#[pyclass(name = "PythonNodeExecutionDomain")]
pub struct PythonNodeExecutionDomain {
    pub(crate) driver: ForeignNodeDriver,
    pub(crate) sequence: Arc<Mutex<u64>>,
    state: Mutex<DomainState>,
    pub(crate) call_timeout: Duration,
    shutdown_timeout: Duration,
}

#[pymethods]
impl PythonNodeExecutionDomain {
    #[new]
    #[pyo3(signature = (node, *, inbox_capacity=16, completion_capacity=16, max_in_flight=1, call_deadline_ms=10_000, shutdown_deadline_ms=5_000, ordering="strict", isolation="in_process"))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        node: Py<PyAny>,
        inbox_capacity: usize,
        completion_capacity: usize,
        max_in_flight: usize,
        call_deadline_ms: u64,
        shutdown_deadline_ms: u64,
        ordering: &str,
        isolation: &str,
    ) -> PyResult<Self> {
        validate_isolation(isolation)?;
        let ordering = match ordering {
            "strict" => ForeignOrdering::Strict,
            "unordered" => ForeignOrdering::Unordered,
            _ => {
                return Err(binding_error(
                    "VOXA-PY-ORDERING",
                    "ordering must be strict or unordered",
                ))
            }
        };
        if max_in_flight != 1 {
            return Err(binding_error(
                "VOXA-PY-IN-FLIGHT",
                "Python V1 executes one task at a time; max_in_flight must be 1",
            ));
        }
        let nz = |value, name| {
            NonZeroUsize::new(value).ok_or_else(|| {
                binding_error("VOXA-PY-CAPACITY", format!("{name} must be non-zero"))
            })
        };
        let call_timeout = Duration::from_millis(call_deadline_ms);
        let shutdown_timeout = Duration::from_millis(shutdown_deadline_ms);
        let config = ForeignDriverConfig {
            command_capacity: nz(inbox_capacity, "inbox_capacity")?,
            command_byte_capacity: NonZeroUsize::new(16 * 1024 * 1024).expect("constant non-zero"),
            completion_capacity: nz(completion_capacity, "completion_capacity")?,
            completion_byte_capacity: NonZeroUsize::new(16 * 1024 * 1024)
                .expect("constant non-zero"),
            max_in_flight: nz(max_in_flight, "max_in_flight")?,
            per_call_deadline: call_timeout,
            shutdown_deadline: shutdown_timeout,
            ordering,
            ..ForeignDriverConfig::default()
        };
        let driver = ForeignNodeDriver::new(config)
            .map_err(|e| binding_error("VOXA-PY-CONFIG", e.to_string()))?;
        let worker_driver = driver.clone();
        let (done_tx, done) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("voxa-python-node".into())
            .spawn(move || {
                run_domain(worker_driver, node, call_timeout);
                let _ = done_tx.send(());
            })
            .map_err(|e| binding_error("VOXA-PY-THREAD", e.to_string()))?;
        Ok(Self {
            driver,
            sequence: Arc::new(Mutex::new(0)),
            state: Mutex::new(DomainState {
                closed: false,
                terminal_callback_completed: false,
                thread: Some(handle),
                done: Some(done),
            }),
            call_timeout,
            shutdown_timeout,
        })
    }

    fn prepare(&self, py: Python<'_>) -> PyResult<()> {
        self.submit(py, ForeignCommandKind::Prepare).map(|_| ())
    }

    fn process(&self, py: Python<'_>, frame: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
        let frame = extract_frame(frame)?;
        let completion = self.submit(py, ForeignCommandKind::Process(frame))?;
        let ForeignCompletionKind::Success { frames, .. } = completion.kind() else {
            unreachable!("failures become Python errors")
        };
        frames
            .iter()
            .cloned()
            .map(|frame| frame_to_python(py, frame))
            .collect()
    }

    fn signal(&self, py: Python<'_>, signal: PyRef<'_, crate::PySignalFrame>) -> PyResult<()> {
        let Frame::Signal(signal) = signal.inner.clone() else {
            unreachable!("typed wrapper invariant")
        };
        self.submit(py, ForeignCommandKind::Signal(signal))
            .map(|_| ())
    }

    fn event(&self, py: Python<'_>, event: PyRef<'_, crate::PyEventFrame>) -> PyResult<()> {
        let Frame::Event(event) = event.inner.clone() else {
            unreachable!("typed wrapper invariant")
        };
        self.submit(py, ForeignCommandKind::Event(event))
            .map(|_| ())
    }

    fn finish(&self, py: Python<'_>) -> PyResult<()> {
        self.submit(py, ForeignCommandKind::Finish)?;
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .terminal_callback_completed = true;
        Ok(())
    }

    fn abort(&self, py: Python<'_>, reason: String) -> PyResult<()> {
        self.submit(
            py,
            ForeignCommandKind::Abort(abort_reason(
                "VOXA-PY-USER-ABORT",
                reason,
                AbortCategory::Cancelled,
            )),
        )?;
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .terminal_callback_completed = true;
        Ok(())
    }

    fn close(&self, py: Python<'_>) -> PyResult<bool> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.closed {
            return Ok(false);
        }
        state.closed = true;
        if !state.terminal_callback_completed || !self.driver.begin_graceful_stop() {
            self.driver.begin_stop(abort_reason(
                "VOXA-PY-CANCELLED",
                "Python domain closed",
                AbortCategory::Cancelled,
            ));
        }
        let done = state
            .done
            .take()
            .expect("only the first close takes the receiver");
        drop(state);
        let completed = py.allow_threads(move || done.recv_timeout(self.shutdown_timeout).is_ok());
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if completed {
            if let Some(handle) = state.thread.take() {
                let _ = handle.join();
            }
            Ok(true)
        } else {
            Err(binding_error("VOXA-PY-SHUTDOWN-DEADLINE", "Python task did not stop before shutdown deadline; in-process thread cannot be killed"))
        }
    }

    #[getter]
    fn is_closed(&self) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).closed
    }
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __exit__(
        &self,
        py: Python<'_>,
        _ty: &Bound<'_, PyAny>,
        _value: &Bound<'_, PyAny>,
        _traceback: &Bound<'_, PyAny>,
    ) {
        let _ = self.close(py);
    }
}

impl PythonNodeExecutionDomain {
    fn submit(&self, py: Python<'_>, kind: ForeignCommandKind) -> PyResult<ForeignCompletion> {
        {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed {
                return Err(binding_error("VOXA-PY-CLOSED", "Python domain is closed"));
            }
        }
        let mut next = self.sequence.lock().unwrap_or_else(|e| e.into_inner());
        let sequence = *next;
        match self
            .driver
            .try_submit(ForeignCommand::new(sequence, kind), Instant::now())
            .map_err(|e| binding_error("VOXA-PY-DRIVER", e.to_string()))?
        {
            ForeignSubmitOutcome::Accepted => {
                *next = next
                    .checked_add(1)
                    .ok_or_else(|| binding_error("VOXA-PY-SEQUENCE", "sequence exhausted"))?;
            }
            ForeignSubmitOutcome::Full => {
                return Err(binding_error(
                    "VOXA-PY-INBOX-FULL",
                    "Python node inbox or in-flight quota is full",
                ))
            }
            ForeignSubmitOutcome::Closed | ForeignSubmitOutcome::Cancelled => {
                return Err(binding_error("VOXA-PY-CLOSED", "Python domain is stopping"))
            }
        }
        drop(next);
        let deadline = Instant::now() + self.call_timeout;
        loop {
            if let Some(completion) = self.driver.try_take_completion() {
                if completion.sequence() != sequence {
                    return Err(binding_error(
                        "VOXA-PY-ORDER",
                        "unexpected completion sequence",
                    ));
                }
                if let ForeignCompletionKind::Failure(reason) = completion.kind() {
                    return Err(binding_error(reason.root().code(), reason.root().message()));
                }
                return Ok(completion);
            }
            if let Some(reason) = self.driver.take_abort_reason() {
                return Err(binding_error(reason.root().code(), reason.root().message()));
            }
            if Instant::now() >= deadline {
                self.driver.expire_deadlines(Instant::now());
                return Err(binding_error(
                    "VOXA-PY-DEADLINE",
                    "Python lifecycle callback exceeded its deadline",
                ));
            }
            py.allow_threads(|| thread::sleep(Duration::from_millis(1)));
        }
    }
}

impl Drop for PythonNodeExecutionDomain {
    fn drop(&mut self) {
        self.driver.begin_stop(abort_reason(
            "VOXA-PY-DROPPED",
            "Python domain dropped",
            AbortCategory::Cancelled,
        ));
    }
}

fn run_domain(driver: ForeignNodeDriver, node: Py<PyAny>, call_timeout: Duration) {
    let loop_object = match Python::with_gil(|py| -> PyResult<Py<PyAny>> {
        let asyncio = py.import("asyncio")?;
        let event_loop = asyncio.call_method0("new_event_loop")?;
        asyncio.call_method1("set_event_loop", (&event_loop,))?;
        Ok(event_loop.unbind())
    }) {
        Ok(value) => value,
        Err(_) => return,
    };

    loop {
        let Some(command) = driver.try_receive() else {
            thread::sleep(Duration::from_millis(1));
            continue;
        };
        let sequence = command.sequence();
        match command.kind().clone() {
            ForeignCommandKind::Stop => break,
            ForeignCommandKind::Cancel => {
                driver.acknowledge_cancel(sequence);
            }
            kind => {
                let result = Python::with_gil(|py| {
                    invoke(py, node.bind(py), loop_object.bind(py), kind, call_timeout)
                });
                let completion = match result {
                    Ok(frames) => ForeignCompletion::success(sequence, frames, [], []),
                    Err(error) => ForeignCompletion::failure(sequence, python_error(error)),
                };
                let outcome = driver.try_complete(completion);
                if outcome == ForeignCompletionOutcome::Full {
                    driver.begin_stop(abort_reason(
                        "VOXA-PY-COMPLETION-FULL",
                        "Python completion mailbox is full",
                        AbortCategory::ForeignException,
                    ));
                }
            }
        }
    }
    Python::with_gil(|py| {
        let _ = loop_object.bind(py).call_method0("close");
    });
}

fn invoke(
    py: Python<'_>,
    node: &Bound<'_, PyAny>,
    event_loop: &Bound<'_, PyAny>,
    kind: ForeignCommandKind,
    timeout: Duration,
) -> PyResult<Vec<Frame>> {
    let (method, argument): (&str, Option<Py<PyAny>>) = match kind {
        ForeignCommandKind::Prepare => ("on_prepare", None),
        ForeignCommandKind::Process(frame) => ("on_process", Some(frame_to_python(py, frame)?)),
        ForeignCommandKind::Signal(signal) => (
            "on_signal",
            Some(frame_to_python(py, Frame::Signal(signal))?),
        ),
        ForeignCommandKind::Event(event) => {
            ("on_event", Some(frame_to_python(py, Frame::Event(event))?))
        }
        ForeignCommandKind::Finish => ("on_finish", None),
        ForeignCommandKind::Abort(reason) => (
            "on_abort",
            Some(
                reason
                    .root()
                    .message()
                    .into_pyobject(py)?
                    .unbind()
                    .into_any(),
            ),
        ),
        ForeignCommandKind::Cancel | ForeignCommandKind::Stop => return Ok(Vec::new()),
    };
    if !node.hasattr(method)? {
        return Ok(Vec::new());
    }
    let result = match argument {
        Some(value) => node.call_method1(method, (value,))?,
        None => node.call_method0(method)?,
    };
    let inspect = py.import("inspect")?;
    let result = if inspect
        .call_method1("isawaitable", (&result,))?
        .is_truthy()?
    {
        let asyncio = py.import("asyncio")?;
        let bounded = asyncio.call_method1("wait_for", (result, timeout.as_secs_f64()))?;
        event_loop.call_method1("run_until_complete", (bounded,))?
    } else {
        result
    };
    normalize_output(&result)
}

fn normalize_output(value: &Bound<'_, PyAny>) -> PyResult<Vec<Frame>> {
    if value.is_none() {
        return Ok(Vec::new());
    }
    if let Ok(list) = value.downcast::<PyList>() {
        return list.iter().map(|item| extract_frame(&item)).collect();
    }
    if let Ok(tuple) = value.downcast::<PyTuple>() {
        return tuple.iter().map(|item| extract_frame(&item)).collect();
    }
    Ok(vec![extract_frame(value)?])
}

fn python_error(error: PyErr) -> AbortReason {
    Python::with_gil(|py| {
        abort_reason(
            "VOXA-PY-EXCEPTION",
            error.value(py).to_string(),
            AbortCategory::ForeignException,
        )
    })
}

fn abort_reason(code: &str, message: impl Into<String>, category: AbortCategory) -> AbortReason {
    let mut message = message.into();
    message.truncate(256);
    AbortReason::new(
        category,
        None,
        AbortStage::Process,
        AbortRootContext::new(code, message, ConfigMap::empty()),
    )
}
