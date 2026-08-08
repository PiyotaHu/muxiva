//! Replaceable structured logging for runtime-facing Muxiva services.

use std::sync::OnceLock;

use muxiva_types::{ErrorCategory, MuxivaError, NodeId, Result, SessionId};

static DEFAULT_LOGGING: OnceLock<()> = OnceLock::new();

const MAX_EVENT_NAME_BYTES: usize = 96;

/// A severity for a structured Muxiva log record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    /// An unrecoverable or user-visible failure.
    Error,
    /// A recoverable condition that needs attention.
    Warn,
    /// A normal runtime lifecycle event.
    Info,
    /// Diagnostic information useful during development.
    Debug,
    /// Highly detailed diagnostic information.
    Trace,
}

/// A structured event that can be emitted through any [`LogSink`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogRecord {
    level: LogLevel,
    event_name: Box<str>,
    session: Option<SessionId>,
    node: Option<NodeId>,
    fields: Vec<(Box<str>, Box<str>)>,
}

impl LogRecord {
    /// Creates a record with its severity and stable event name.
    ///
    /// Event names are lowercase ASCII dotted identifiers such as
    /// `runtime.started`. Each dot-separated segment starts with a lowercase
    /// letter and may then contain lowercase letters, digits, or hyphens. Names
    /// are limited to 96 bytes and validation failures use `MUXIVA-LOG-002`.
    pub fn new(level: LogLevel, event_name: impl Into<Box<str>>) -> Result<Self> {
        let event_name = event_name.into();
        if !is_stable_event_name(&event_name) {
            return Err(MuxivaError::new(
                ErrorCategory::Validation,
                "MUXIVA-LOG-002",
                "event name must be a stable lowercase ASCII dotted identifier",
            ));
        }

        Ok(Self {
            level,
            event_name,
            session: None,
            node: None,
            fields: Vec::new(),
        })
    }

    /// Attaches the session associated with the event.
    pub fn with_session(mut self, session: SessionId) -> Self {
        self.session = Some(session);
        self
    }

    /// Attaches the graph node associated with the event.
    pub fn with_node(mut self, node: NodeId) -> Self {
        self.node = Some(node);
        self
    }

    /// Adds an ordered, non-sensitive field to the record.
    ///
    /// This is fallible so callers cannot accidentally log reserved field names.
    pub fn with_field(
        mut self,
        name: impl Into<Box<str>>,
        value: impl Into<Box<str>>,
    ) -> Result<Self> {
        let name = name.into();
        if is_reserved_field(&name) {
            return Err(MuxivaError::new(
                ErrorCategory::Validation,
                "MUXIVA-LOG-001",
                "log field name is reserved",
            ));
        }

        self.fields.push((name, value.into()));
        Ok(self)
    }

    /// Returns the record severity.
    pub const fn level(&self) -> LogLevel {
        self.level
    }

    /// Returns the stable event name.
    pub fn event_name(&self) -> &str {
        &self.event_name
    }

    /// Returns the associated session, if any.
    pub fn session(&self) -> Option<&SessionId> {
        self.session.as_ref()
    }

    /// Returns the associated graph node, if any.
    pub fn node(&self) -> Option<&NodeId> {
        self.node.as_ref()
    }

    /// Returns the record fields in insertion order.
    pub fn fields(&self) -> &[(Box<str>, Box<str>)] {
        &self.fields
    }
}

/// Receives structured log records without coupling callers to a logging backend.
pub trait LogSink: Send + Sync {
    /// Emits a structured record.
    fn emit(&self, record: &LogRecord);
}

/// A [`LogSink`] implementation backed by the `tracing` ecosystem.
#[derive(Clone, Copy, Debug, Default)]
pub struct TracingLogSink;

impl LogSink for TracingLogSink {
    fn emit(&self, record: &LogRecord) {
        match record.level() {
            LogLevel::Error => tracing::error!(
                event = %record.event_name(),
                session = ?record.session(),
                node = ?record.node(),
                fields = ?record.fields(),
                "Muxiva event"
            ),
            LogLevel::Warn => tracing::warn!(
                event = %record.event_name(),
                session = ?record.session(),
                node = ?record.node(),
                fields = ?record.fields(),
                "Muxiva event"
            ),
            LogLevel::Info => tracing::info!(
                event = %record.event_name(),
                session = ?record.session(),
                node = ?record.node(),
                fields = ?record.fields(),
                "Muxiva event"
            ),
            LogLevel::Debug => tracing::debug!(
                event = %record.event_name(),
                session = ?record.session(),
                node = ?record.node(),
                fields = ?record.fields(),
                "Muxiva event"
            ),
            LogLevel::Trace => tracing::trace!(
                event = %record.event_name(),
                session = ?record.session(),
                node = ?record.node(),
                fields = ?record.fields(),
                "Muxiva event"
            ),
        }
    }
}

/// Initializes the default `tracing` formatter once without replacing an existing subscriber.
pub fn init_default_logging() -> Result<()> {
    DEFAULT_LOGGING.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .try_init();
    });

    Ok(())
}

fn is_reserved_field(name: &str) -> bool {
    ["payload", "authorization", "private_extension"]
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
}

fn is_stable_event_name(name: &str) -> bool {
    (1..=MAX_EVENT_NAME_BYTES).contains(&name.len())
        && name.is_ascii()
        && name.split('.').all(|segment| {
            let mut bytes = segment.bytes();
            matches!(bytes.next(), Some(byte) if byte.is_ascii_lowercase())
                && bytes
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && !segment.ends_with('-')
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{LogLevel, LogRecord, LogSink};
    use muxiva_types::{NodeId, SessionId};

    #[derive(Default)]
    struct CollectSink {
        records: Mutex<Vec<LogRecord>>,
    }

    impl LogSink for CollectSink {
        fn emit(&self, record: &LogRecord) {
            self.records
                .lock()
                .expect("collecting sink lock")
                .push(record.clone());
        }
    }

    #[test]
    fn custom_sink_receives_structured_record() {
        let sink = CollectSink::default();
        let record = LogRecord::new(LogLevel::Info, "runtime.started")
            .expect("stable event name")
            .with_session(SessionId::new("session-1").expect("valid session"))
            .with_field("worker_count", "2")
            .expect("safe field");

        sink.emit(&record);

        assert_eq!(
            sink.records
                .lock()
                .expect("collecting sink lock")
                .as_slice(),
            &[record]
        );
    }

    #[test]
    fn record_preserves_identity_and_field_insertion_order() {
        let record = LogRecord::new(LogLevel::Warn, "runtime.degraded")
            .expect("stable event name")
            .with_session(SessionId::new("session-1").expect("valid session"))
            .with_node(NodeId::new("asr.primary").expect("valid node"))
            .with_field("attempt", "2")
            .expect("safe field")
            .with_field("reason", "timeout")
            .expect("safe field");

        assert_eq!(record.level(), LogLevel::Warn);
        assert_eq!(record.event_name(), "runtime.degraded");
        assert_eq!(record.session().map(SessionId::as_str), Some("session-1"));
        assert_eq!(record.node().map(NodeId::as_str), Some("asr.primary"));
        assert_eq!(
            record.fields(),
            &[
                (Box::<str>::from("attempt"), Box::<str>::from("2")),
                (Box::<str>::from("reason"), Box::<str>::from("timeout"),)
            ]
        );
    }

    #[test]
    fn rejects_payload_field_to_prevent_sensitive_logging() {
        let error = LogRecord::new(LogLevel::Info, "runtime.started")
            .expect("stable event name")
            .with_field("payload", "audio bytes")
            .expect_err("payload must be rejected");

        assert_eq!(error.code(), "MUXIVA-LOG-001");
    }

    #[test]
    fn rejects_authorization_field_to_prevent_sensitive_logging() {
        let error = LogRecord::new(LogLevel::Info, "runtime.started")
            .expect("stable event name")
            .with_field("authorization", "Bearer secret")
            .expect_err("authorization must be rejected");

        assert_eq!(error.code(), "MUXIVA-LOG-001");
    }

    #[test]
    fn rejects_private_extension_field_to_prevent_sensitive_logging() {
        let error = LogRecord::new(LogLevel::Info, "runtime.started")
            .expect("stable event name")
            .with_field("private_extension", "secret")
            .expect_err("private extension must be rejected");

        assert_eq!(error.code(), "MUXIVA-LOG-001");
    }

    #[test]
    fn rejects_unstable_event_names() {
        let invalid_names = [
            "".to_owned(),
            " runtime.started".to_owned(),
            "runtime.started ".to_owned(),
            "runtime\nstarted".to_owned(),
            "runtime.开始".to_owned(),
            "runtime started".to_owned(),
            "Runtime.started".to_owned(),
            "runtime..started".to_owned(),
            "runtime.-started".to_owned(),
            "runtime.started-".to_owned(),
            format!("runtime.{}", "a".repeat(89)),
        ];

        for name in invalid_names {
            let error = LogRecord::new(LogLevel::Info, name)
                .expect_err("unstable event name must be rejected");

            assert_eq!(error.code(), "MUXIVA-LOG-002");
        }
    }

    #[test]
    fn default_logging_initialization_is_idempotent() {
        super::init_default_logging().expect("first initialization");
        super::init_default_logging().expect("second initialization");
    }
}
