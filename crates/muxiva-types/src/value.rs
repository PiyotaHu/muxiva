use std::collections::BTreeMap;

use crate::{ErrorCategory, FrameBuffer, MuxivaError, Result};

/// A finite floating-point value suitable for cross-language metadata.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct FiniteF64(f64);

impl FiniteF64 {
    /// Creates a finite floating-point value.
    pub fn new(value: f64) -> Result<Self> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(MuxivaError::new(
                ErrorCategory::Validation,
                "MUXIVA-FRM-VALUE-NUMBER",
                "value must be a finite number",
            ))
        }
    }

    /// Returns the finite floating-point value.
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Eq for FiniteF64 {}

/// A closed, owned value algebra for metadata and extension values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    /// An absent value.
    Null,
    /// A boolean value.
    Bool(bool),
    /// A signed integer value.
    Integer(i64),
    /// A finite floating-point value.
    Float(FiniteF64),
    /// An owned UTF-8 string.
    String(Box<str>),
    /// Immutable owned bytes.
    Bytes(FrameBuffer),
    /// An ordered owned list.
    List(Box<[Value]>),
    /// An ordered, validated string-keyed map.
    Map(ValueMap),
}

/// A deterministic map of validated string keys to values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValueMap(BTreeMap<Box<str>, Value>);

impl ValueMap {
    /// Creates an empty value map.
    pub fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Creates a map after validating every key and rejecting duplicates.
    pub fn try_from_iter<I, K>(values: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, Value)>,
        K: Into<Box<str>>,
    {
        Ok(Self(collect_values(values)?))
    }

    /// Returns the value associated with `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Iterates over entries in deterministic key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(key, value)| (key.as_ref(), value))
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Immutable, deterministic metadata for a frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Metadata(BTreeMap<Box<str>, Value>);

impl Metadata {
    /// Creates empty metadata.
    pub fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Creates metadata after validating every key and rejecting duplicates.
    pub fn try_from_iter<I, K>(values: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, Value)>,
        K: Into<Box<str>>,
    {
        Ok(Self(collect_values(values)?))
    }

    /// Returns the value associated with `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Iterates over entries in deterministic key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(key, value)| (key.as_ref(), value))
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

fn collect_values<I, K>(values: I) -> Result<BTreeMap<Box<str>, Value>>
where
    I: IntoIterator<Item = (K, Value)>,
    K: Into<Box<str>>,
{
    let mut map = BTreeMap::new();
    for (key, value) in values {
        let key = key.into();
        validate_key(&key)?;
        if map.insert(key, value).is_some() {
            return Err(invalid_key_error());
        }
    }
    Ok(map)
}

fn validate_key(key: &str) -> Result<()> {
    if !key.is_empty() && key.len() <= 255 && !key.bytes().any(|byte| byte.is_ascii_control()) {
        Ok(())
    } else {
        Err(invalid_key_error())
    }
}

fn invalid_key_error() -> MuxivaError {
    MuxivaError::new(
        ErrorCategory::Validation,
        "MUXIVA-FRM-VALUE-KEY",
        "value map key must be non-empty, at most 255 bytes, and contain no ASCII controls",
    )
}

#[cfg(test)]
mod tests {
    use super::{FiniteF64, Metadata, Value, ValueMap};
    use crate::FrameBuffer;

    #[test]
    fn values_reject_non_finite_numbers_and_bad_keys() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                FiniteF64::new(value).unwrap_err().code(),
                "MUXIVA-FRM-VALUE-NUMBER"
            );
        }

        for key in ["", "contains\u{0000}", &"a".repeat(256)] {
            let error = ValueMap::try_from_iter([(key, Value::Null)]).unwrap_err();
            assert_eq!(error.code(), "MUXIVA-FRM-VALUE-KEY");
        }
    }

    #[test]
    fn values_cover_every_variant() {
        let map = ValueMap::try_from_iter([("key", Value::Null)]).unwrap();
        let values = [
            Value::Null,
            Value::Bool(true),
            Value::Integer(-7),
            Value::Float(FiniteF64::new(1.5).unwrap()),
            Value::String(Box::from("text")),
            Value::Bytes(FrameBuffer::from_vec(vec![1, 2])),
            Value::List(Box::new([Value::Bool(false)])),
            Value::Map(map),
        ];

        assert_eq!(values.len(), 8);
    }

    #[test]
    fn maps_and_metadata_have_deterministic_iteration_and_reject_duplicates() {
        let values = [("z", Value::Integer(1)), ("a", Value::Integer(2))];
        let map = ValueMap::try_from_iter(values.clone()).unwrap();
        let metadata = Metadata::try_from_iter(values).unwrap();

        assert_eq!(
            map.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(
            metadata.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(map.get("a"), Some(&Value::Integer(2)));
        assert!(Metadata::empty().is_empty());
        assert_eq!(
            Metadata::try_from_iter([("same", Value::Null), ("same", Value::Bool(true))])
                .unwrap_err()
                .code(),
            "MUXIVA-FRM-VALUE-KEY"
        );
    }
}
