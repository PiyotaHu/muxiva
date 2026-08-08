use std::error::Error;

use crate::{NodeId, SessionId, StreamId};

/// Classifies a Muxiva failure for callers that need stable error handling.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ErrorCategory {
    /// A supplied configuration is missing or invalid.
    Configuration,
    /// An input or requested operation failed validation.
    Validation,
    /// A lifecycle transition could not be completed.
    Lifecycle,
    /// Work was cancelled before completion.
    Cancelled,
    /// A dependency outside Muxiva failed.
    External,
    /// Muxiva encountered an unexpected internal failure.
    Internal,
}

/// Adds structured, non-display context to a [`MuxivaError`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ErrorContext {
    /// The session associated with the failure.
    Session(SessionId),
    /// The graph node associated with the failure.
    Node(NodeId),
    /// The stream associated with the failure.
    Stream(StreamId),
    /// The phase in which the failure occurred.
    Phase(Box<str>),
    /// A named contextual detail.
    Detail { key: Box<str>, value: Box<str> },
}

/// The reason an error code was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("error code must start with MUXIVA-, be 6 through 64 bytes, and contain only uppercase ASCII letters, digits, and hyphens")]
pub struct ErrorCodeError;

/// A structured Muxiva error with a stable code and optional source error.
#[derive(Debug, thiserror::Error)]
#[error("[{code}] {message}")]
pub struct MuxivaError {
    category: ErrorCategory,
    code: Box<str>,
    message: Box<str>,
    contexts: Vec<ErrorContext>,
    #[source]
    cause: Option<Box<dyn Error + Send + Sync + 'static>>,
}

/// The result type used by Muxiva public APIs.
pub type Result<T> = std::result::Result<T, MuxivaError>;

impl MuxivaError {
    /// Creates an error whose stable code has been validated.
    ///
    /// Panics if `code` is not a valid stable Muxiva code. Use [`Self::try_new`]
    /// when handling invalid input is required.
    pub fn new(
        category: ErrorCategory,
        code: impl Into<Box<str>>,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self::try_new(category, code, message).expect("Muxiva error code must be valid")
    }

    /// Attempts to create an error after validating its stable code.
    pub fn try_new(
        category: ErrorCategory,
        code: impl Into<Box<str>>,
        message: impl Into<Box<str>>,
    ) -> std::result::Result<Self, ErrorCodeError> {
        let code = code.into();
        if !is_valid_code(&code) {
            return Err(ErrorCodeError);
        }

        Ok(Self {
            category,
            code,
            message: message.into(),
            contexts: Vec::new(),
            cause: None,
        })
    }

    /// Attaches the node associated with this error.
    pub fn with_node(mut self, node: NodeId) -> Self {
        self.contexts.push(ErrorContext::Node(node));
        self
    }

    /// Attaches the phase in which this error occurred.
    pub fn with_phase(mut self, phase: impl Into<Box<str>>) -> Self {
        self.contexts.push(ErrorContext::Phase(phase.into()));
        self
    }

    /// Attaches a named detail to this error.
    pub fn with_context(mut self, key: impl Into<Box<str>>, value: impl Into<Box<str>>) -> Self {
        self.contexts.push(ErrorContext::Detail {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Attaches an underlying source error.
    pub fn with_source<E>(mut self, source: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        self.cause = Some(Box::new(source));
        self
    }

    /// Returns this error's stable category.
    pub const fn category(&self) -> ErrorCategory {
        self.category
    }

    /// Returns this error's stable code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns this error's human-readable message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns contextual data in the order it was added.
    pub fn contexts(&self) -> &[ErrorContext] {
        &self.contexts
    }
}

fn is_valid_code(code: &str) -> bool {
    (6..=64).contains(&code.len())
        && code.starts_with("MUXIVA-")
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::{ErrorCategory, ErrorContext, MuxivaError};
    use crate::{ErrorCodeError, NodeId};

    #[test]
    fn error_preserves_category_code_and_context() {
        let node = NodeId::new("mock-asr").unwrap();
        let error = MuxivaError::new(
            ErrorCategory::Configuration,
            "MUXIVA-CFG-001",
            "missing model",
        )
        .with_node(node.clone())
        .with_phase("prepare")
        .with_context("config_key", "model");

        assert_eq!(error.category(), ErrorCategory::Configuration);
        assert_eq!(error.code(), "MUXIVA-CFG-001");
        assert_eq!(error.message(), "missing model");
        assert!(error.to_string().contains("MUXIVA-CFG-001"));
        assert_eq!(error.contexts().len(), 3);
        assert_eq!(error.contexts()[0], ErrorContext::Node(node));
    }

    #[test]
    fn error_rejects_invalid_stable_code() {
        assert!(
            MuxivaError::try_new(ErrorCategory::Internal, "temporary code", "failure").is_err()
        );
    }

    #[test]
    fn error_code_error_is_reachable_from_the_crate_root() {
        let result: std::result::Result<MuxivaError, ErrorCodeError> =
            MuxivaError::try_new(ErrorCategory::Internal, "temporary code", "failure");

        assert_eq!(result.unwrap_err(), ErrorCodeError);
    }

    #[test]
    fn error_display_omits_sensitive_context_values() {
        let error = MuxivaError::new(
            ErrorCategory::Configuration,
            "MUXIVA-CFG-001",
            "missing model",
        )
        .with_context("api_token", "secret-value");

        assert!(!error.to_string().contains("secret-value"));
    }

    #[test]
    fn error_exposes_its_source() {
        let error = MuxivaError::new(
            ErrorCategory::External,
            "MUXIVA-EXT-001",
            "dependency failed",
        )
        .with_source(std::io::Error::other("connection reset"));

        assert!(std::error::Error::source(&error).is_some());
    }
}
