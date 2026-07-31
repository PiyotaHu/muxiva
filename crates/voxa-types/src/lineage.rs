use crate::{EdgeId, ErrorCategory, FrameId, NodeId, Result, VoxaError};

/// Attributes a transformation to a node, an edge, or both.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransformOrigin {
    node_id: Option<NodeId>,
    edge_id: Option<EdgeId>,
}

impl TransformOrigin {
    /// Creates an origin with at least one attribution.
    pub fn new(node_id: Option<NodeId>, edge_id: Option<EdgeId>) -> Result<Self> {
        if node_id.is_none() && edge_id.is_none() {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-LINEAGE-ORIGIN",
                "lineage origin must identify a node or edge",
            ));
        }

        Ok(Self { node_id, edge_id })
    }

    /// Returns the attributed node, if any.
    pub fn node_id(&self) -> Option<&NodeId> {
        self.node_id.as_ref()
    }

    /// Returns the attributed edge, if any.
    pub fn edge_id(&self) -> Option<&EdgeId> {
        self.edge_id.as_ref()
    }
}

/// An immutable record of a transformation from a parent frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineageEntry {
    parent_frame_id: FrameId,
    origin: TransformOrigin,
    reason: Box<str>,
}

impl LineageEntry {
    /// Creates a lineage entry with a bounded, control-character-free reason.
    pub fn new(
        parent_frame_id: FrameId,
        origin: TransformOrigin,
        reason: impl Into<Box<str>>,
    ) -> Result<Self> {
        let reason = reason.into();
        Self::validate_reason(&reason)?;

        Ok(Self {
            parent_frame_id,
            origin,
            reason,
        })
    }

    /// Returns the parent frame identifier.
    pub fn parent_frame_id(&self) -> &FrameId {
        &self.parent_frame_id
    }

    /// Returns the transformation origin.
    pub fn origin(&self) -> &TransformOrigin {
        &self.origin
    }

    /// Returns the operation reason.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub(crate) fn validate_reason(reason: &str) -> Result<()> {
        if reason.is_empty()
            || reason.len() > 256
            || reason.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(VoxaError::new(
                ErrorCategory::Validation,
                "VOXA-FRM-LINEAGE-REASON",
                "lineage reason must be non-empty, at most 256 bytes, and contain no ASCII controls",
            ));
        }
        Ok(())
    }
}

/// Immutable, ordered transformation history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lineage(Box<[LineageEntry]>);

impl Lineage {
    /// Creates empty lineage for a source frame.
    pub fn empty() -> Self {
        Self(Box::new([]))
    }

    /// Iterates over lineage entries in derivation order.
    pub fn iter(&self) -> impl Iterator<Item = &LineageEntry> {
        self.0.iter()
    }

    /// Returns the number of lineage entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether this is source-frame lineage.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn from_entries(entries: Vec<LineageEntry>) -> Self {
        Self(entries.into_boxed_slice())
    }

    pub(crate) fn append(self, entry: LineageEntry) -> Self {
        let mut entries = self.0.into_vec();
        entries.push(entry);
        Self::from_entries(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::{Lineage, LineageEntry, TransformOrigin};
    use crate::{EdgeId, FrameId, NodeId};

    fn origin() -> TransformOrigin {
        TransformOrigin::new(Some(NodeId::new("normalizer").unwrap()), None).unwrap()
    }

    fn entry(parent: &str, reason: &str) -> LineageEntry {
        LineageEntry::new(FrameId::new(parent).unwrap(), origin(), reason).unwrap()
    }

    #[test]
    fn transform_origin_requires_attribution() {
        let error = TransformOrigin::new(None, None).unwrap_err();
        assert_eq!(error.code(), "VOXA-FRM-LINEAGE-ORIGIN");
    }

    #[test]
    fn transform_origin_allows_node_edge_or_both() {
        let node = NodeId::new("node").unwrap();
        let edge = EdgeId::new("edge").unwrap();

        assert!(TransformOrigin::new(Some(node.clone()), None).is_ok());
        assert!(TransformOrigin::new(None, Some(edge.clone())).is_ok());
        let both = TransformOrigin::new(Some(node), Some(edge)).unwrap();
        assert_eq!(both.node_id().unwrap().as_str(), "node");
        assert_eq!(both.edge_id().unwrap().as_str(), "edge");
    }

    #[test]
    fn lineage_reasons_are_bounded_and_safe_for_diagnostics() {
        for reason in [
            Box::<str>::from(""),
            "x".repeat(257).into(),
            "bad\nreason".into(),
        ] {
            let error =
                LineageEntry::new(FrameId::new("parent").unwrap(), origin(), reason).unwrap_err();
            assert_eq!(error.code(), "VOXA-FRM-LINEAGE-REASON");
        }
    }

    #[test]
    fn lineage_preserves_entry_order_when_constructed_or_appended_internally() {
        let lineage = Lineage::from_entries(vec![entry("first", "normalize")])
            .append(entry("second", "resample"));
        assert_eq!(
            lineage
                .iter()
                .map(|entry| entry.parent_frame_id().as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }
}
