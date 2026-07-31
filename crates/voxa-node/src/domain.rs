use std::{
    num::NonZeroUsize,
    sync::Mutex,
    time::{Duration, Instant},
};

use napi::{
    threadsafe_function::{
        ErrorStrategy, ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode,
    },
    Error, JsFunction, Result, Status,
};
use napi_derive::napi;
use voxa_core::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigMap, ForeignCommand,
    ForeignCommandKind, ForeignCompletion, ForeignCompletionKind, ForeignCompletionOutcome,
    ForeignDriverConfig, ForeignNodeDriver, ForeignSubmitOutcome,
};

use crate::frame::owned_text_frame;

#[derive(Clone)]
struct Command {
    sequence: i64,
    kind: String,
    payload_json: Option<String>,
}

#[napi(object)]
pub struct JsDomainCommand {
    pub sequence: i64,
    pub kind: String,
    pub payload_json: Option<String>,
}

/// Native, bounded bridge used inside a dedicated JS Worker. Core's
/// `ForeignNodeDriver` is the sole admission, completion, stop, and abort owner;
/// the TSFN only schedules accepted owned commands onto the JS event loop.
#[napi]
pub struct NodeExecutionDomain {
    callback: ThreadsafeFunction<Command, ErrorStrategy::Fatal>,
    driver: ForeignNodeDriver,
    responses: Mutex<std::collections::BTreeMap<u64, String>>,
}

fn abort_reason(code: &str, message: &str) -> AbortReason {
    AbortReason::new(
        AbortCategory::ForeignException,
        None,
        AbortStage::Runtime,
        AbortRootContext::new(code, message, ConfigMap::empty()),
    )
}

fn command(sequence: u64, kind: &str, payload: Option<String>) -> Result<ForeignCommand> {
    let kind = match kind {
        "prepare" => ForeignCommandKind::Prepare,
        "process" => ForeignCommandKind::Process(owned_text_frame(
            payload.unwrap_or_default(),
            sequence as i64,
        )?),
        "signal" => ForeignCommandKind::Process(owned_text_frame(
            payload.unwrap_or_default(),
            sequence as i64,
        )?),
        "finish" => ForeignCommandKind::Finish,
        "abort" => {
            ForeignCommandKind::Abort(abort_reason("VOXA-NODE-ABORT", "TypeScript node aborted"))
        }
        _ => return Err(Error::new(Status::InvalidArg, "unknown lifecycle command")),
    };
    Ok(ForeignCommand::new(sequence, kind))
}

#[napi]
impl NodeExecutionDomain {
    #[napi(constructor)]
    pub fn new(callback: JsFunction, capacity: u32) -> Result<Self> {
        if !(1..=65_536).contains(&capacity) {
            return Err(Error::new(
                Status::InvalidArg,
                "capacity must be between 1 and 65536",
            ));
        }
        let callback = callback.create_threadsafe_function(
            capacity as usize,
            |ctx: ThreadSafeCallContext<Command>| {
                Ok(vec![JsDomainCommand {
                    sequence: ctx.value.sequence,
                    kind: ctx.value.kind,
                    payload_json: ctx.value.payload_json,
                }])
            },
        )?;
        let capacity = NonZeroUsize::new(capacity as usize).expect("validated non-zero");
        let driver = ForeignNodeDriver::new(ForeignDriverConfig {
            command_capacity: capacity,
            command_byte_capacity: NonZeroUsize::new(16 * 1024 * 1024).unwrap(),
            completion_capacity: capacity,
            completion_byte_capacity: NonZeroUsize::new(16 * 1024 * 1024).unwrap(),
            max_in_flight: capacity,
            per_call_deadline: Duration::from_secs(30),
            shutdown_deadline: Duration::from_secs(5),
            ..ForeignDriverConfig::default()
        })
        .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?;
        Ok(Self {
            callback,
            driver,
            responses: Mutex::new(std::collections::BTreeMap::new()),
        })
    }

    #[napi]
    pub fn submit(
        &self,
        sequence: i64,
        kind: String,
        payload_json: Option<String>,
    ) -> Result<String> {
        let sequence_u64 = u64::try_from(sequence)
            .map_err(|_| Error::new(Status::InvalidArg, "sequence must be non-negative"))?;
        let owned = command(sequence_u64, &kind, payload_json.clone())?;
        let outcome = self
            .driver
            .try_submit(owned, Instant::now())
            .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?;
        if outcome != ForeignSubmitOutcome::Accepted {
            return Ok(match outcome {
                ForeignSubmitOutcome::Full => "full",
                ForeignSubmitOutcome::Closed | ForeignSubmitOutcome::Cancelled => "closed",
                ForeignSubmitOutcome::Accepted => unreachable!(),
            }
            .into());
        }
        // Mark Core's owned command as dispatched before scheduling its JS view.
        let _ = self.driver.try_receive();
        let status = self.callback.call(
            Command {
                sequence,
                kind,
                payload_json,
            },
            ThreadsafeFunctionCallMode::NonBlocking,
        );
        if status != Status::Ok {
            self.driver.begin_stop(abort_reason(
                "VOXA-NODE-TSFN",
                "ThreadsafeFunction is full or closing",
            ));
            return Ok(if status == Status::QueueFull {
                "full"
            } else {
                "closed"
            }
            .into());
        }
        Ok("accepted".into())
    }

    /// JS posts only owned serialized output; Core validates and budgets it as
    /// an immutable TextFrame before it becomes observable to the caller.
    #[napi]
    pub fn complete(&self, sequence: i64, completion_json: String) -> Result<bool> {
        let sequence = u64::try_from(sequence)
            .map_err(|_| Error::new(Status::InvalidArg, "sequence must be non-negative"))?;
        let output = owned_text_frame(completion_json.clone(), sequence as i64)?;
        let accepted = self.driver.try_complete(ForeignCompletion::success(
            sequence,
            vec![output],
            vec![],
            vec![],
        )) == ForeignCompletionOutcome::Accepted;
        if accepted {
            self.responses
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(sequence, completion_json);
        }
        Ok(accepted)
    }

    #[napi]
    pub fn fail(
        &self,
        sequence: i64,
        code: String,
        message: String,
        completion_json: String,
    ) -> Result<bool> {
        let sequence = u64::try_from(sequence)
            .map_err(|_| Error::new(Status::InvalidArg, "sequence must be non-negative"))?;
        self.responses
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(sequence, completion_json);
        Ok(self.driver.try_complete(ForeignCompletion::failure(
            sequence,
            abort_reason(&code, &message),
        )) == ForeignCompletionOutcome::Accepted)
    }

    #[napi]
    pub fn drain_completions(&self) -> Vec<String> {
        let mut output = Vec::new();
        while let Some(completion) = self.driver.try_take_completion() {
            if let ForeignCompletionKind::Success { .. } = completion.kind() {
                if let Some(response) = self
                    .responses
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&completion.sequence())
                {
                    output.push(response);
                }
            }
        }
        // Failure seals the driver and is surfaced exactly once.
        if self.driver.take_abort_reason().is_some() {
            let mut responses = self.responses.lock().unwrap_or_else(|e| e.into_inner());
            output.extend(std::mem::take(&mut *responses).into_values());
        }
        output
    }

    #[napi(getter)]
    pub fn outstanding(&self) -> u32 {
        self.driver.snapshot().in_flight.min(u32::MAX as usize) as u32
    }
    #[napi]
    pub fn close(&self) -> bool {
        let first = self.driver.begin_stop(abort_reason(
            "VOXA-NODE-STOPPED",
            "Node execution domain stopped",
        ));
        if first {
            let _ = self.callback.clone().abort();
        }
        first
    }
}
