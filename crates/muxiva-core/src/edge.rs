use std::{collections::BTreeSet, num::NonZeroUsize};

use muxiva_types::{EdgeId, FrameType, NodeId, Value};

use crate::node::{ConfigKey, DescriptorNameError, PortName};

/// A stable name that resolves an Edge policy implementation outside the graph definition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EdgePolicyName(Box<str>);

impl EdgePolicyName {
    /// Creates a stable policy registry name.
    pub fn new(value: impl Into<Box<str>>) -> std::result::Result<Self, DescriptorNameError> {
        let value = value.into();
        validate_name(&value)?;
        Ok(Self(value))
    }

    /// Returns the registry name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable Studio/log visibility label.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VisibilityLabel(Box<str>);

impl VisibilityLabel {
    /// Creates a stable visibility label.
    pub fn new(value: impl Into<Box<str>>) -> std::result::Result<Self, DescriptorNameError> {
        let value = value.into();
        validate_name(&value)?;
        Ok(Self(value))
    }

    /// Returns the label text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_name(value: &str) -> std::result::Result<(), DescriptorNameError> {
    if value.is_empty() {
        return Err(DescriptorNameError::Empty);
    }
    if value.len() > 255 {
        return Err(DescriptorNameError::TooLong);
    }
    if value.trim() != value {
        return Err(DescriptorNameError::LeadingOrTrailingWhitespace);
    }
    if value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(DescriptorNameError::ContainsControlCharacter);
    }
    Ok(())
}

/// Declarative behavior when a future bounded queue is full.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum QueueOverflowPolicy {
    /// Preserve frames by applying backpressure.
    Block,
    /// Discard the oldest queued frame.
    DropOldest,
    /// Discard the newly arriving frame.
    DropNewest,
    /// Abort graph execution.
    Abort,
}

/// Stable queue configuration. Stage 4A does not allocate this queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuePolicy {
    capacity: NonZeroUsize,
    overflow: QueueOverflowPolicy,
}

impl QueuePolicy {
    /// Creates a bounded queue descriptor.
    pub const fn new(capacity: NonZeroUsize, overflow: QueueOverflowPolicy) -> Self {
        Self { capacity, overflow }
    }

    /// Returns the declared frame capacity.
    pub const fn capacity(self) -> NonZeroUsize {
        self.capacity
    }

    /// Returns the declared queue-full behavior.
    pub const fn overflow(self) -> QueueOverflowPolicy {
        self.overflow
    }
}

impl Default for QueuePolicy {
    fn default() -> Self {
        Self::new(NonZeroUsize::MIN, QueueOverflowPolicy::Block)
    }
}

/// Behavior after an Edge validator rejects a frame.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ValidationFailureAction {
    /// Drop the frame and record the reason.
    Drop,
    /// Abort the whole graph.
    Abort,
}

/// Pure-data validation policy selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationPolicy {
    /// Perform only the mandatory exact type gate.
    TypeGateOnly,
    /// Resolve a named validator from a separate runtime registry.
    Named {
        /// Stable registry name.
        policy: EdgePolicyName,
        /// Action taken when validation rejects a frame.
        on_failure: ValidationFailureAction,
    },
}

/// Pure-data transform policy selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransformPolicy {
    /// Forward the same immutable frame.
    Identity,
    /// Resolve a named transform from a separate runtime registry.
    Named(EdgePolicyName),
}

/// Declarative condition controlling whether an Edge is enabled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnabledCondition {
    /// The Edge is always active.
    Always,
    /// The Edge is disabled without deleting its definition.
    Never,
    /// The Edge is active when one node configuration value equals `expected`.
    ConfigEquals {
        /// Node whose configuration is read.
        node_id: NodeId,
        /// Configuration key to compare.
        key: ConfigKey,
        /// Expected stable value.
        expected: Value,
    },
}

/// Default presentation visibility for an Edge.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VisibilityLevel {
    /// Visible in default diagnostics and Studio views.
    Public,
    /// Hidden from default diagnostics and Studio views.
    Private,
}

/// Deterministic presentation labels that carry no execution behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisibilityDescriptor {
    level: VisibilityLevel,
    labels: Box<[VisibilityLabel]>,
}

impl VisibilityDescriptor {
    /// Creates visibility data, sorting and de-duplicating labels.
    pub fn new(level: VisibilityLevel, labels: impl IntoIterator<Item = VisibilityLabel>) -> Self {
        let labels = labels.into_iter().collect::<BTreeSet<_>>();
        Self {
            level,
            labels: labels.into_iter().collect(),
        }
    }

    /// Returns the default visibility level.
    pub const fn level(&self) -> VisibilityLevel {
        self.level
    }

    /// Returns labels in stable lexical order.
    pub fn labels(&self) -> &[VisibilityLabel] {
        &self.labels
    }
}

impl Default for VisibilityDescriptor {
    fn default() -> Self {
        Self::new(VisibilityLevel::Public, [])
    }
}

/// A pure, stable and exactly typed graph connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeDescriptor {
    edge_id: EdgeId,
    from_node_id: NodeId,
    from_output_port: PortName,
    to_node_id: NodeId,
    to_input_port: PortName,
    frame_type: FrameType,
    queue_policy: QueuePolicy,
    validation_policy: ValidationPolicy,
    transform_policy: TransformPolicy,
    enabled: EnabledCondition,
    visibility: VisibilityDescriptor,
}

impl EdgeDescriptor {
    /// Creates an explicit Edge descriptor. GraphBuilder validates its endpoints.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        edge_id: EdgeId,
        from_node_id: NodeId,
        from_output_port: PortName,
        to_node_id: NodeId,
        to_input_port: PortName,
        frame_type: FrameType,
        queue_policy: QueuePolicy,
        validation_policy: ValidationPolicy,
        transform_policy: TransformPolicy,
        enabled: EnabledCondition,
        visibility: VisibilityDescriptor,
    ) -> Self {
        Self {
            edge_id,
            from_node_id,
            from_output_port,
            to_node_id,
            to_input_port,
            frame_type,
            queue_policy,
            validation_policy,
            transform_policy,
            enabled,
            visibility,
        }
    }

    /// Returns the stable Edge ID.
    pub fn edge_id(&self) -> &EdgeId {
        &self.edge_id
    }

    /// Returns the explicit source node.
    pub fn from_node_id(&self) -> &NodeId {
        &self.from_node_id
    }

    /// Returns the explicit source output port.
    pub fn from_output_port(&self) -> &PortName {
        &self.from_output_port
    }

    /// Returns the explicit target node.
    pub fn to_node_id(&self) -> &NodeId {
        &self.to_node_id
    }

    /// Returns the explicit target input port.
    pub fn to_input_port(&self) -> &PortName {
        &self.to_input_port
    }

    /// Returns the exact transported frame type.
    pub const fn frame_type(&self) -> FrameType {
        self.frame_type
    }

    /// Returns future queue policy data.
    pub const fn queue_policy(&self) -> QueuePolicy {
        self.queue_policy
    }

    /// Returns validation policy data.
    pub const fn validation_policy(&self) -> &ValidationPolicy {
        &self.validation_policy
    }

    /// Returns transform policy data.
    pub const fn transform_policy(&self) -> &TransformPolicy {
        &self.transform_policy
    }

    /// Returns the enabled condition.
    pub const fn enabled(&self) -> &EnabledCondition {
        &self.enabled
    }

    /// Returns presentation visibility data.
    pub const fn visibility(&self) -> &VisibilityDescriptor {
        &self.visibility
    }
}

/// Read-only per-Edge metrics shape used by future runtime snapshots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeMetricsSnapshot {
    edge_id: EdgeId,
    queue_capacity: usize,
    queue_len: usize,
    high_watermark: usize,
    enqueue_total: u64,
    dequeue_total: u64,
    drop_total: u64,
    signal_total: u64,
    full_total: u64,
    blocked_duration_ns: u64,
    oldest_frame_age_ns: Option<u64>,
    payload_bytes_total: u64,
    audio_duration_ns_total: u64,
    latest_error_reason: Option<Box<str>>,
}

/// Stage 4 name for the immutable per-Edge metrics data shape.
///
/// Runtime mutation and subscription belong to later stages; callers observe
/// this data through snapshots.
pub type EdgeMetrics = EdgeMetricsSnapshot;

impl EdgeMetricsSnapshot {
    /// Creates a zero-valued snapshot for one declared Edge.
    pub fn zero(edge_id: EdgeId, queue_capacity: usize) -> Self {
        Self {
            edge_id,
            queue_capacity,
            queue_len: 0,
            high_watermark: 0,
            enqueue_total: 0,
            dequeue_total: 0,
            drop_total: 0,
            signal_total: 0,
            full_total: 0,
            blocked_duration_ns: 0,
            oldest_frame_age_ns: None,
            payload_bytes_total: 0,
            audio_duration_ns_total: 0,
            latest_error_reason: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_runtime(
        edge_id: EdgeId,
        queue_capacity: usize,
        queue_len: usize,
        high_watermark: usize,
        enqueue_total: u64,
        dequeue_total: u64,
        drop_total: u64,
        signal_total: u64,
        full_total: u64,
        blocked_duration_ns: u64,
        oldest_frame_age_ns: Option<u64>,
        payload_bytes_total: u64,
        audio_duration_ns_total: u64,
        latest_error_reason: Option<Box<str>>,
    ) -> Self {
        Self {
            edge_id,
            queue_capacity,
            queue_len,
            high_watermark,
            enqueue_total,
            dequeue_total,
            drop_total,
            signal_total,
            full_total,
            blocked_duration_ns,
            oldest_frame_age_ns,
            payload_bytes_total,
            audio_duration_ns_total,
            latest_error_reason,
        }
    }

    /// Returns the Edge identity.
    pub fn edge_id(&self) -> &EdgeId {
        &self.edge_id
    }

    /// Returns declared queue capacity.
    pub const fn queue_capacity(&self) -> usize {
        self.queue_capacity
    }

    /// Returns current queue length.
    pub const fn queue_len(&self) -> usize {
        self.queue_len
    }

    /// Returns the maximum observed queue length.
    pub const fn high_watermark(&self) -> usize {
        self.high_watermark
    }

    /// Returns successful enqueue count.
    pub const fn enqueue_total(&self) -> u64 {
        self.enqueue_total
    }

    /// Returns successful dequeue count.
    pub const fn dequeue_total(&self) -> u64 {
        self.dequeue_total
    }

    /// Returns dropped frame count.
    pub const fn drop_total(&self) -> u64 {
        self.drop_total
    }

    /// Returns the number of Stage 4 policy signals observed on this Edge.
    ///
    /// Signals are counted but are not delivered to adjacent nodes until the
    /// Stage 6 signal-routing contract is implemented.
    pub const fn signal_total(&self) -> u64 {
        self.signal_total
    }

    /// Returns queue-full observation count.
    pub const fn full_total(&self) -> u64 {
        self.full_total
    }

    /// Returns accumulated blocked time in nanoseconds.
    pub const fn blocked_duration_ns(&self) -> u64 {
        self.blocked_duration_ns
    }

    /// Returns age of the oldest queued frame in nanoseconds when present.
    pub const fn oldest_frame_age_ns(&self) -> Option<u64> {
        self.oldest_frame_age_ns
    }

    /// Returns cumulative payload bytes accepted by this Edge.
    pub const fn payload_bytes_total(&self) -> u64 {
        self.payload_bytes_total
    }

    /// Returns cumulative audio media duration accepted by this Edge.
    pub const fn audio_duration_ns_total(&self) -> u64 {
        self.audio_duration_ns_total
    }

    /// Returns the latest non-sensitive error reason.
    pub fn latest_error_reason(&self) -> Option<&str> {
        self.latest_error_reason.as_deref()
    }

    pub(crate) fn record_delivery(&mut self) {
        self.enqueue_total = self.enqueue_total.saturating_add(1);
        self.dequeue_total = self.dequeue_total.saturating_add(1);
    }

    pub(crate) fn record_drop(&mut self, reason: &str) {
        self.drop_total = self.drop_total.saturating_add(1);
        self.set_latest_reason(reason);
    }

    pub(crate) fn record_signal(&mut self) {
        self.signal_total = self.signal_total.saturating_add(1);
    }

    pub(crate) fn record_error(&mut self, reason: &str) {
        self.set_latest_reason(reason);
    }

    fn set_latest_reason(&mut self, reason: &str) {
        const MAX_REASON_BYTES: usize = 256;
        let mut sanitized = String::new();
        for character in reason.chars() {
            let character = if character.is_ascii_control() {
                ' '
            } else {
                character
            };
            if sanitized.len() + character.len_utf8() > MAX_REASON_BYTES {
                break;
            }
            sanitized.push(character);
        }
        self.latest_error_reason = Some(sanitized.into_boxed_str());
    }
}
