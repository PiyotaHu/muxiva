use std::{cmp::Ordering, fmt};

use crate::{
    ClockDomainId, ErrorCategory, Extensions, FrameId, FrameType, Lineage, Metadata, Result,
    SequenceId, StreamId, Timestamp, TraceId, VoxaError,
};

/// Describes how timestamps in a clock domain are interpreted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClockKind {
    /// A clock that advances monotonically.
    Monotonic,
    /// A timeline relative to a media source.
    MediaRelative,
    /// A civil wall clock.
    WallClock,
}

/// A clock identity and its interpretation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClockDomain {
    id: ClockDomainId,
    kind: ClockKind,
}

impl ClockDomain {
    /// Creates a clock domain.
    pub fn new(id: ClockDomainId, kind: ClockKind) -> Self {
        Self { id, kind }
    }

    /// Returns the clock domain identity.
    pub fn id(&self) -> &ClockDomainId {
        &self.id
    }

    /// Returns the clock interpretation.
    pub const fn kind(&self) -> ClockKind {
        self.kind
    }
}

/// Immutable identity, timing, routing, and diagnostic data shared by a frame.
#[derive(Clone, Eq, PartialEq)]
pub struct FrameHeader {
    frame_id: FrameId,
    timestamp: Timestamp,
    clock_domain: ClockDomain,
    sequence_id: SequenceId,
    stream_id: StreamId,
    trace_id: TraceId,
    frame_type: FrameType,
    metadata: Metadata,
    extensions: Extensions,
    lineage: Lineage,
}

impl FrameHeader {
    /// Creates a header after validating its direct lineage.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        frame_id: FrameId,
        timestamp: Timestamp,
        clock_domain: ClockDomain,
        sequence_id: SequenceId,
        stream_id: StreamId,
        trace_id: TraceId,
        frame_type: FrameType,
        metadata: Metadata,
        extensions: Extensions,
        lineage: Lineage,
    ) -> Result<Self> {
        if lineage
            .iter()
            .any(|entry| entry.parent_frame_id() == &frame_id)
        {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-LINEAGE-CYCLE",
                "a frame cannot name itself as a lineage parent",
            ));
        }

        Ok(Self {
            frame_id,
            timestamp,
            clock_domain,
            sequence_id,
            stream_id,
            trace_id,
            frame_type,
            metadata,
            extensions,
            lineage,
        })
    }

    /// Returns the frame identity.
    pub fn frame_id(&self) -> &FrameId {
        &self.frame_id
    }

    /// Returns the timestamp scalar interpreted by [`Self::clock_domain`].
    pub const fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the timestamp's clock domain.
    pub fn clock_domain(&self) -> &ClockDomain {
        &self.clock_domain
    }

    /// Returns the sequence counter within the stream.
    pub const fn sequence_id(&self) -> SequenceId {
        self.sequence_id
    }

    /// Returns the stream identity.
    pub fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    /// Returns the trace identity.
    pub fn trace_id(&self) -> &TraceId {
        &self.trace_id
    }

    /// Returns the declared payload type.
    pub const fn frame_type(&self) -> FrameType {
        self.frame_type
    }

    /// Returns immutable frame metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Returns immutable versioned extensions.
    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    /// Returns immutable transformation history.
    pub fn lineage(&self) -> &Lineage {
        &self.lineage
    }

    /// Orders timestamps only when both headers use the same complete clock domain.
    pub fn compare_timestamp(&self, other: &FrameHeader) -> Result<Ordering> {
        if self.clock_domain != other.clock_domain {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-CLOCK-DOMAIN",
                "timestamps from different clock domains cannot be ordered",
            )
            .with_context(
                "left_clock_domain_id",
                Box::<str>::from(self.clock_domain.id.as_str()),
            )
            .with_context(
                "right_clock_domain_id",
                Box::<str>::from(other.clock_domain.id.as_str()),
            ));
        }

        Ok(self.timestamp.as_nanos().cmp(&other.timestamp.as_nanos()))
    }
}

impl fmt::Debug for FrameHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameHeader")
            .field("frame_id", &self.frame_id)
            .field("timestamp", &self.timestamp)
            .field("clock_domain", &self.clock_domain)
            .field("sequence_id", &self.sequence_id)
            .field("stream_id", &self.stream_id)
            .field("trace_id", &self.trace_id)
            .field("frame_type", &self.frame_type)
            .field("metadata_count", &self.metadata.len())
            .field("extension_count", &self.extensions.len())
            .field("lineage_count", &self.lineage.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{ClockDomain, ClockKind, FrameHeader};
    use crate::{
        ClockDomainId, Extension, ExtensionProducer, ExtensionVisibility, Extensions, FrameId,
        FrameType, Lineage, LineageEntry, Metadata, NamespacedName, NodeId, SchemaVersion,
        SequenceId, StreamId, Timestamp, TraceId, TransformOrigin, Value,
    };

    fn header_with_lineage(frame_id: FrameId, lineage: Lineage) -> crate::Result<FrameHeader> {
        FrameHeader::new(
            frame_id,
            Timestamp::from_nanos(0),
            ClockDomain::new(
                ClockDomainId::new("capture.audio").unwrap(),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(0),
            StreamId::new("stream-1").unwrap(),
            TraceId::new("trace-1").unwrap(),
            FrameType::Audio,
            Metadata::empty(),
            Extensions::empty(),
            lineage,
        )
    }

    #[test]
    fn header_rejects_self_parent_lineage() {
        let frame_id = FrameId::new("frame-cycle").unwrap();
        let origin = TransformOrigin::new(Some(NodeId::new("normalize").unwrap()), None).unwrap();
        let entry = LineageEntry::new(frame_id.clone(), origin, "normalize").unwrap();
        let lineage = Lineage::from_entries(vec![entry]);
        let error = header_with_lineage(frame_id, lineage).unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-LINEAGE-CYCLE");
    }

    #[test]
    fn header_debug_omits_metadata_extensions_and_lineage_contents() {
        let origin = TransformOrigin::new(Some(NodeId::new("normalize").unwrap()), None).unwrap();
        let lineage = Lineage::from_entries(vec![LineageEntry::new(
            FrameId::new("parent-frame").unwrap(),
            origin,
            "secret lineage reason",
        )
        .unwrap()]);
        let metadata = Metadata::try_from_iter([(
            "secret_metadata_key",
            Value::String("secret metadata".into()),
        )])
        .unwrap();
        let extensions = Extensions::try_from_iter([Extension::new(
            NamespacedName::new("com.example.secret_extension").unwrap(),
            SchemaVersion::new(1).unwrap(),
            ExtensionProducer::Core,
            ExtensionVisibility::Private,
            Value::String("secret extension value".into()),
        )])
        .unwrap();
        let header = FrameHeader::new(
            FrameId::new("frame-safe-debug").unwrap(),
            Timestamp::from_nanos(7),
            ClockDomain::new(
                ClockDomainId::new("capture.audio").unwrap(),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(2),
            StreamId::new("stream-1").unwrap(),
            TraceId::new("trace-1").unwrap(),
            FrameType::Audio,
            metadata,
            extensions,
            lineage,
        )
        .unwrap();

        let debug = format!("{header:?}");
        assert!(debug.contains("metadata_count: 1"));
        assert!(debug.contains("extension_count: 1"));
        assert!(debug.contains("lineage_count: 1"));
        for secret in [
            "secret_metadata_key",
            "secret metadata",
            "com.example.secret_extension",
            "secret extension value",
            "secret lineage reason",
        ] {
            assert!(!debug.contains(secret));
        }
    }
}
