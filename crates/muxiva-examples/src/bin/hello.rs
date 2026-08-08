#![forbid(unsafe_code)]

use muxiva_core::logging::{init_default_logging, LogLevel, LogRecord, LogSink, TracingLogSink};
use muxiva_examples::hello_message;
use muxiva_types::{ErrorCategory, MuxivaError, Result, SessionId};

fn main() -> Result<()> {
    let session = SessionId::new("hello-session").map_err(|error| {
        MuxivaError::new(
            ErrorCategory::Validation,
            "MUXIVA-EXM-001",
            "hello session identifier must be valid",
        )
        .with_source(error)
    })?;

    init_default_logging()?;
    init_default_logging()?;

    let record = LogRecord::new(LogLevel::Info, "runtime.ready")?
        .with_session(session.clone())
        .with_field("example", "hello")?;
    TracingLogSink.emit(&record);

    println!("{}", hello_message(&session));
    Ok(())
}
