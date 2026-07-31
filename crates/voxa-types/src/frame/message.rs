use std::fmt;

use crate::{
    ErrorCategory, FrameBuffer, NamespacedName, NodeId, Result, SchemaVersion, Value, VoxaError,
};

/// Immutable owned UTF-8 text.
#[derive(Clone, Eq, PartialEq)]
pub struct TextData(Box<str>);

impl TextData {
    /// Creates text from an already-valid Rust string.
    pub fn new(text: impl Into<Box<str>>) -> Self {
        Self(text.into())
    }

    /// Validates UTF-8 bytes and copies them into owned string storage.
    pub fn from_utf8(bytes: FrameBuffer) -> Result<Self> {
        let text = std::str::from_utf8(bytes.as_slice()).map_err(|_| {
            VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-TEXT-UTF8",
                "text payload must contain valid UTF-8",
            )
        })?;
        Ok(Self(Box::from(text)))
    }

    /// Returns the owned text as a borrowed string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for TextData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextData")
            .field("byte_len", &self.0.len())
            .finish()
    }
}

/// A validated owned media type and subtype.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MediaType(Box<str>);

impl MediaType {
    /// Creates a media type after validating its restricted token grammar.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self> {
        let value = value.into();
        if !is_valid_media_type(&value) {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-MEDIA-TYPE",
                "media type must contain one valid ASCII type/subtype pair",
            ));
        }
        Ok(Self(value))
    }

    /// Returns the media type and subtype.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_valid_media_type(value: &str) -> bool {
    if !(1..=127).contains(&value.len()) || !value.is_ascii() {
        return false;
    }
    let Some((type_name, subtype)) = value.split_once('/') else {
        return false;
    };
    !type_name.is_empty()
        && !subtype.is_empty()
        && type_name.bytes().all(is_media_token_byte)
        && subtype.bytes().all(is_media_token_byte)
}

fn is_media_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
        )
}

/// Immutable opaque bytes with an optional validated media type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteData {
    buffer: FrameBuffer,
    media_type: Option<MediaType>,
}

impl ByteData {
    /// Creates an opaque byte payload. Empty buffers are valid.
    pub fn new(buffer: FrameBuffer, media_type: Option<MediaType>) -> Self {
        Self { buffer, media_type }
    }

    /// Returns the immutable byte buffer.
    pub fn buffer(&self) -> &FrameBuffer {
        &self.buffer
    }

    /// Returns the optional media type.
    pub fn media_type(&self) -> Option<&MediaType> {
        self.media_type.as_ref()
    }
}

/// An owned graph-local signal payload.
#[derive(Clone, Eq, PartialEq)]
pub struct SignalData {
    name: NamespacedName,
    schema_version: SchemaVersion,
    source: NodeId,
    payload: Value,
}

impl SignalData {
    /// Creates a signal from already-validated component values.
    pub fn new(
        name: NamespacedName,
        schema_version: SchemaVersion,
        source: NodeId,
        payload: Value,
    ) -> Self {
        Self {
            name,
            schema_version,
            source,
            payload,
        }
    }

    /// Returns the signal name.
    pub fn name(&self) -> &NamespacedName {
        &self.name
    }

    /// Returns the signal schema version.
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// Returns the source node.
    pub fn source(&self) -> &NodeId {
        &self.source
    }

    /// Returns the owned structured payload.
    pub fn payload(&self) -> &Value {
        &self.payload
    }
}

/// An owned published event payload.
#[derive(Clone, Eq, PartialEq)]
pub struct EventData {
    topic: NamespacedName,
    schema_version: SchemaVersion,
    source: NodeId,
    payload: Value,
}

impl EventData {
    /// Creates an event from already-validated component values.
    pub fn new(
        topic: NamespacedName,
        schema_version: SchemaVersion,
        source: NodeId,
        payload: Value,
    ) -> Self {
        Self {
            topic,
            schema_version,
            source,
            payload,
        }
    }

    /// Returns the event topic.
    pub fn topic(&self) -> &NamespacedName {
        &self.topic
    }

    /// Returns the event schema version.
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// Returns the source node.
    pub fn source(&self) -> &NodeId {
        &self.source
    }

    /// Returns the owned structured payload.
    pub fn payload(&self) -> &Value {
        &self.payload
    }
}
