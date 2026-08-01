use std::{
    num::NonZeroUsize,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use pyo3::{
    prelude::*,
    types::{PyDict, PyList, PyTuple},
};
use voxa_core::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigMap, ForeignCommand,
    ForeignCommandKind, ForeignCompletion, ForeignCompletionEmission, ForeignCompletionKind,
    ForeignCompletionOutcome, ForeignDriverConfig, ForeignNodeDriver, ForeignOrdering,
    ForeignSubmitOutcome, PortName,
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
    graph_output_ports: Arc<Mutex<Option<Vec<PortName>>>>,
}

#[pymethods]
impl PythonNodeExecutionDomain {
    #[new]
    #[pyo3(signature = (node, *, inbox_capacity=16, completion_capacity=16, max_in_flight=1, call_deadline_ms=10_000, shutdown_deadline_ms=5_000, ordering="strict", isolation="in_process"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
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
        let graph_output_ports = Arc::new(Mutex::new(None));
        let worker_output_ports = Arc::clone(&graph_output_ports);
        let (done_tx, done) = mpsc::channel();
        let handle = thread::Builder::new()
            .name("voxa-python-node".into())
            .spawn(move || {
                run_domain(worker_driver, node, call_timeout, worker_output_ports);
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
            graph_output_ports,
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
        let completed = py.detach(move || done.recv_timeout(self.shutdown_timeout).is_ok());
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
    pub(crate) fn new_graph(node: Py<PyAny>, output_ports: Vec<PortName>) -> PyResult<Self> {
        let domain = Self::new(node, 16, 16, 1, 10_000, 5_000, "strict", "in_process")?;
        *domain
            .graph_output_ports
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(output_ports);
        Ok(domain)
    }

    pub(crate) fn submit_blocking(
        &self,
        kind: ForeignCommandKind,
    ) -> voxa_types::Result<ForeignCompletion> {
        {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.closed {
                return Err(domain_error("VOXA-PY-CLOSED", "Python domain is closed"));
            }
        }
        let mut next = self.sequence.lock().unwrap_or_else(|e| e.into_inner());
        let sequence = *next;
        match self
            .driver
            .try_submit(ForeignCommand::new(sequence, kind), Instant::now())
            .map_err(|error| domain_error("VOXA-PY-DRIVER", error.to_string()))?
        {
            ForeignSubmitOutcome::Accepted => {
                *next = next
                    .checked_add(1)
                    .ok_or_else(|| domain_error("VOXA-PY-SEQUENCE", "sequence exhausted"))?;
            }
            ForeignSubmitOutcome::Full => {
                return Err(domain_error(
                    "VOXA-PY-INBOX-FULL",
                    "Python node inbox or in-flight quota is full",
                ));
            }
            ForeignSubmitOutcome::Closed | ForeignSubmitOutcome::Cancelled => {
                return Err(domain_error("VOXA-PY-CLOSED", "Python domain is stopping"));
            }
        }
        drop(next);
        let deadline = Instant::now() + self.call_timeout;
        loop {
            if let Some(completion) = self.driver.try_take_completion() {
                if completion.sequence() != sequence {
                    return Err(domain_error(
                        "VOXA-PY-ORDER",
                        "unexpected completion sequence",
                    ));
                }
                if let ForeignCompletionKind::Failure(reason) = completion.kind() {
                    return Err(domain_error(reason.root().code(), reason.root().message()));
                }
                return Ok(completion);
            }
            if let Some(reason) = self.driver.take_abort_reason() {
                return Err(domain_error(reason.root().code(), reason.root().message()));
            }
            if Instant::now() >= deadline {
                self.driver.expire_deadlines(Instant::now());
                return Err(domain_error(
                    "VOXA-PY-DEADLINE",
                    "Python lifecycle callback exceeded its deadline",
                ));
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    pub(crate) fn mark_terminal_callback_completed(&self) {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .terminal_callback_completed = true;
    }

    pub(crate) fn close_blocking(&self) -> voxa_types::Result<bool> {
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
        let completed = done.recv_timeout(self.shutdown_timeout).is_ok();
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if completed {
            if let Some(handle) = state.thread.take() {
                let _ = handle.join();
            }
            Ok(true)
        } else {
            Err(domain_error(
                "VOXA-PY-SHUTDOWN-DEADLINE",
                "Python task did not stop before shutdown deadline; in-process thread cannot be killed",
            ))
        }
    }

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
            py.detach(|| thread::sleep(Duration::from_millis(1)));
        }
    }
}

fn domain_error(code: &str, message: impl AsRef<str>) -> voxa_types::VoxaError {
    voxa_types::VoxaError::try_new(
        voxa_types::ErrorCategory::External,
        code.to_owned(),
        message.as_ref().to_owned(),
    )
    .unwrap_or_else(|_| {
        voxa_types::VoxaError::new(
            voxa_types::ErrorCategory::External,
            "VOXA-PY-DOMAIN",
            "Python execution domain failure",
        )
    })
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

fn run_domain(
    driver: ForeignNodeDriver,
    node: Py<PyAny>,
    call_timeout: Duration,
    graph_output_ports: Arc<Mutex<Option<Vec<PortName>>>>,
) {
    let loop_object = match Python::attach(|py| -> PyResult<Py<PyAny>> {
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
                let result = Python::attach(|py| {
                    let ports = graph_output_ports
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .clone();
                    invoke(
                        py,
                        node.bind(py),
                        loop_object.bind(py),
                        kind,
                        call_timeout,
                        ports.as_deref(),
                    )
                });
                let completion = match result {
                    Ok(InvocationOutput::Frames(frames)) => {
                        ForeignCompletion::success(sequence, frames, [], [])
                    }
                    Ok(InvocationOutput::Emissions(emissions)) => {
                        ForeignCompletion::success_with_emissions(sequence, emissions, [], [])
                    }
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
    Python::attach(|py| {
        let _ = loop_object.bind(py).call_method0("close");
    });
}

fn invoke(
    py: Python<'_>,
    node: &Bound<'_, PyAny>,
    event_loop: &Bound<'_, PyAny>,
    kind: ForeignCommandKind,
    timeout: Duration,
    graph_output_ports: Option<&[PortName]>,
) -> PyResult<InvocationOutput> {
    let (method, arguments): (&str, Vec<Py<PyAny>>) = match kind {
        ForeignCommandKind::Prepare => ("on_prepare", Vec::new()),
        ForeignCommandKind::ProcessSource => ("on_process", Vec::new()),
        ForeignCommandKind::Process(frame) => ("on_process", vec![frame_to_python(py, frame)?]),
        ForeignCommandKind::ProcessOnPort { frame, input_port } => {
            let port = input_port.as_str().into_pyobject(py)?.unbind().into_any();
            ("on_process", vec![frame_to_python(py, frame)?, port])
        }
        ForeignCommandKind::Signal(signal) => (
            "on_signal",
            vec![frame_to_python(py, Frame::Signal(signal))?],
        ),
        ForeignCommandKind::Event(event) => {
            ("on_event", vec![frame_to_python(py, Frame::Event(event))?])
        }
        ForeignCommandKind::Finish => ("on_finish", Vec::new()),
        ForeignCommandKind::Abort(reason) => (
            "on_abort",
            vec![reason
                .root()
                .message()
                .into_pyobject(py)?
                .unbind()
                .into_any()],
        ),
        ForeignCommandKind::Cancel | ForeignCommandKind::Stop => {
            return Ok(match graph_output_ports {
                Some(_) => InvocationOutput::Emissions(Vec::new()),
                None => InvocationOutput::Frames(Vec::new()),
            })
        }
    };
    if !node.hasattr(method)? {
        return Ok(match graph_output_ports {
            Some(_) => InvocationOutput::Emissions(Vec::new()),
            None => InvocationOutput::Frames(Vec::new()),
        });
    }
    let inspect = py.import("inspect")?;
    let arguments = if arguments.len() == 2 {
        let callback = node.getattr(method)?;
        let parameters = inspect
            .call_method1("signature", (callback,))?
            .getattr("parameters")?;
        if parameters.len()? < 2 {
            arguments.into_iter().take(1).collect()
        } else {
            arguments
        }
    } else {
        arguments
    };
    let result = if arguments.is_empty() {
        node.call_method0(method)?
    } else {
        node.call_method(method, PyTuple::new(py, arguments)?, None)?
    };
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
    match graph_output_ports {
        Some(output_ports) => {
            normalize_graph_output(&result, output_ports).map(InvocationOutput::Emissions)
        }
        None => normalize_output(&result).map(InvocationOutput::Frames),
    }
}

enum InvocationOutput {
    Frames(Vec<Frame>),
    Emissions(Vec<ForeignCompletionEmission>),
}

fn normalize_graph_output(
    value: &Bound<'_, PyAny>,
    output_ports: &[PortName],
) -> PyResult<Vec<ForeignCompletionEmission>> {
    if value.is_none() {
        return Ok(Vec::new());
    }
    if let Ok(mapping) = value.cast::<PyDict>() {
        let mut emissions = Vec::new();
        for (key, value) in mapping.iter() {
            let name = key.extract::<String>()?;
            let port = output_ports
                .iter()
                .find(|port| port.as_str() == name)
                .ok_or_else(|| {
                    binding_error(
                        "VOXA-PY-GRAPH-OUTPUT-PORT",
                        format!("callback emitted undeclared output port {name}"),
                    )
                })?;
            emissions.extend(
                normalize_output(&value)?
                    .into_iter()
                    .map(|frame| ForeignCompletionEmission::new(port.clone(), frame)),
            );
        }
        return Ok(emissions);
    }
    if output_ports.len() != 1 {
        return Err(binding_error(
            "VOXA-PY-GRAPH-OUTPUT",
            "a callback with zero or multiple output ports must return a dict of port names to frames",
        ));
    }
    Ok(normalize_output(value)?
        .into_iter()
        .map(|frame| ForeignCompletionEmission::new(output_ports[0].clone(), frame))
        .collect())
}

fn normalize_output(value: &Bound<'_, PyAny>) -> PyResult<Vec<Frame>> {
    if value.is_none() {
        return Ok(Vec::new());
    }
    if let Ok(list) = value.cast::<PyList>() {
        return list.iter().map(|item| extract_frame(&item)).collect();
    }
    if let Ok(tuple) = value.cast::<PyTuple>() {
        return tuple.iter().map(|item| extract_frame(&item)).collect();
    }
    Ok(vec![extract_frame(value)?])
}

fn python_error(error: PyErr) -> AbortReason {
    Python::attach(|py| {
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
