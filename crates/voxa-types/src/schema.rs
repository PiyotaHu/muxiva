use std::{fmt, str::FromStr};

use crate::{ErrorCategory, Result, VoxaError};

/// A non-zero version for a frame schema.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
    /// Creates a non-zero schema version.
    pub fn new(value: u32) -> Result<Self> {
        if value == 0 {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-SCHEMA-VERSION",
                "schema version must be non-zero",
            ));
        }

        Ok(Self(value))
    }

    /// Returns the schema version value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A qualified ASCII namespace name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NamespacedName(Box<str>);

impl NamespacedName {
    /// Creates a namespaced name after validating its grammar.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self> {
        let value = value.into();
        validate_namespace(&value)?;
        Ok(Self(value))
    }

    /// Returns the qualified namespace name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NamespacedName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for NamespacedName {
    type Err = VoxaError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

fn validate_namespace(value: &str) -> Result<()> {
    let is_valid = (3..=255).contains(&value.len())
        && value.is_ascii()
        && value.split('.').count() >= 2
        && value.split('.').all(is_valid_namespace_segment);

    if is_valid {
        Ok(())
    } else {
        Err(VoxaError::new(
            ErrorCategory::Validation,
            "VOXA-FRM-NAMESPACE",
            "name must be a qualified ASCII namespace",
        ))
    }
}

fn is_valid_namespace_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };

    first.is_ascii_alphanumeric()
        && !matches!(segment.as_bytes().last(), Some(b'_' | b'-'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::{NamespacedName, SchemaVersion};
    use crate::ErrorCategory;

    #[test]
    fn schema_version_rejects_zero() {
        let version = SchemaVersion::new(7).expect("non-zero schema version");
        assert_eq!(version.get(), 7);

        let error = SchemaVersion::new(0).expect_err("zero must be rejected");
        assert_eq!(error.category(), ErrorCategory::Validation);
        assert_eq!(error.code(), "VOXA-FRM-SCHEMA-VERSION");
    }

    #[test]
    fn namespace_accepts_segment_edges() {
        for value in [
            "a.b",
            "com.example.trace",
            "team.flow_pressure",
            "voxa.turn.interrupted",
            "A-1.b_2",
        ] {
            let name = NamespacedName::new(value).expect("valid namespace");
            assert_eq!(name.as_str(), value);
        }
    }

    #[test]
    fn namespace_rejects_every_grammar_failure_with_one_stable_code() {
        for value in [
            "",
            "ab",
            "single",
            ".example",
            "com..example",
            "com.example.",
            "com._example",
            "com.example_",
            "com.example-",
            "com.ex!ample",
            "com.ex\u{00e4}mple",
            &"a".repeat(256),
        ] {
            let error = NamespacedName::new(value).expect_err("invalid namespace");
            assert_eq!(error.category(), ErrorCategory::Validation);
            assert_eq!(error.code(), "VOXA-FRM-NAMESPACE");
            assert_eq!(error.message(), "name must be a qualified ASCII namespace");
        }
    }
}
