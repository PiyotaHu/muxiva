use std::{collections::BTreeMap, error::Error, fmt, time::Duration};

use muxiva_types::{EventFrame, Frame, FrameType, NodeId, Result, SignalFrame, Value};

use crate::{EventBus, PublishReport, ResourceStore};

/// The role a node has in a graph.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NodeKind {
    /// Produces frames and therefore has no input ports.
    Source,
    /// Consumes frames and may produce replacement or derived frames.
    Transform,
    /// Consumes frames and therefore has no output ports.
    Sink,
}

/// A stable, validated port name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PortName(Box<str>);

/// A stable registered node type name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeTypeName(Box<str>);

/// A stable configuration key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ConfigKey(Box<str>);

/// The reason a descriptor name was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorNameError {
    /// The name has no bytes.
    Empty,
    /// The name exceeds the 255-byte protocol limit.
    TooLong,
    /// The name begins or ends with whitespace.
    LeadingOrTrailingWhitespace,
    /// The name contains an ASCII control character.
    ContainsControlCharacter,
}

impl fmt::Display for DescriptorNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("descriptor name must not be empty"),
            Self::TooLong => formatter.write_str("descriptor name must be at most 255 bytes"),
            Self::LeadingOrTrailingWhitespace => {
                formatter.write_str("descriptor name must not have leading or trailing whitespace")
            }
            Self::ContainsControlCharacter => {
                formatter.write_str("descriptor name must not contain ASCII control characters")
            }
        }
    }
}

impl Error for DescriptorNameError {}

fn validate_descriptor_name(value: &str) -> std::result::Result<(), DescriptorNameError> {
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

macro_rules! descriptor_name {
    ($name:ident) => {
        impl $name {
            /// Creates a validated stable name.
            pub fn new(
                value: impl Into<Box<str>>,
            ) -> std::result::Result<Self, DescriptorNameError> {
                let value = value.into();
                validate_descriptor_name(&value)?;
                Ok(Self(value))
            }

            /// Returns the name as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

descriptor_name!(PortName);
descriptor_name!(NodeTypeName);
descriptor_name!(ConfigKey);

/// Whether a port receives or emits frames.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PortDirection {
    /// A frame entry point.
    Input,
    /// A frame emission point.
    Output,
}

/// One explicitly named, exactly typed node port.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortDescriptor {
    node_id: NodeId,
    name: PortName,
    direction: PortDirection,
    frame_type: FrameType,
}

impl PortDescriptor {
    /// Describes an explicit port. There is no `Any` frame type.
    pub fn new(
        node_id: NodeId,
        name: PortName,
        direction: PortDirection,
        frame_type: FrameType,
    ) -> Self {
        Self {
            node_id,
            name,
            direction,
            frame_type,
        }
    }

    /// Returns the owning node ID.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns the stable port name.
    pub fn name(&self) -> &PortName {
        &self.name
    }

    /// Returns whether this port is an input or output.
    pub const fn direction(&self) -> PortDirection {
        self.direction
    }

    /// Returns the one exact accepted frame type.
    pub const fn frame_type(&self) -> FrameType {
        self.frame_type
    }
}

/// Pure-data configuration schema metadata for a registered node type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigSchema(Value);

impl ConfigSchema {
    /// Creates schema metadata from the closed Muxiva [`Value`] algebra.
    pub fn new(value: Value) -> Self {
        Self(value)
    }

    /// Creates an empty map schema.
    pub fn empty() -> Self {
        Self(Value::Map(muxiva_types::ValueMap::empty()))
    }

    /// Returns the schema data.
    pub const fn value(&self) -> &Value {
        &self.0
    }
}

impl Default for ConfigSchema {
    fn default() -> Self {
        Self::empty()
    }
}

/// Declarative metadata about which lifecycle hooks an implementation uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleCapabilities {
    prepare: bool,
    process: bool,
    finish: bool,
    abort: bool,
}

impl LifecycleCapabilities {
    /// Declares hook support. `process` must be true for a usable node.
    pub const fn new(prepare: bool, process: bool, finish: bool, abort: bool) -> Self {
        Self {
            prepare,
            process,
            finish,
            abort,
        }
    }

    /// Returns whether `on_prepare` performs implementation work.
    pub const fn prepare(self) -> bool {
        self.prepare
    }

    /// Returns whether `on_process` is implemented.
    pub const fn process(self) -> bool {
        self.process
    }

    /// Returns whether `on_finish` performs implementation work.
    pub const fn finish(self) -> bool {
        self.finish
    }

    /// Returns whether `on_abort` performs implementation work.
    pub const fn abort(self) -> bool {
        self.abort
    }
}

impl Default for LifecycleCapabilities {
    fn default() -> Self {
        Self::new(false, true, false, false)
    }
}

/// Stable node registration and port data stored in a graph definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeDescriptor {
    node_id: NodeId,
    node_type: NodeTypeName,
    kind: NodeKind,
    ports: Box<[PortDescriptor]>,
    config_schema: ConfigSchema,
    lifecycle: LifecycleCapabilities,
}

impl NodeDescriptor {
    /// Creates a pure-data node descriptor.
    pub fn new(
        node_id: NodeId,
        node_type: NodeTypeName,
        kind: NodeKind,
        ports: impl Into<Box<[PortDescriptor]>>,
        config_schema: ConfigSchema,
        lifecycle: LifecycleCapabilities,
    ) -> Self {
        Self {
            node_id,
            node_type,
            kind,
            ports: ports.into(),
            config_schema,
            lifecycle,
        }
    }

    /// Returns the stable graph-local node ID.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns the stable registry type name.
    pub fn node_type(&self) -> &NodeTypeName {
        &self.node_type
    }

    /// Returns the node role.
    pub const fn kind(&self) -> NodeKind {
        self.kind
    }

    /// Returns ports in descriptor order.
    pub fn ports(&self) -> &[PortDescriptor] {
        &self.ports
    }

    /// Returns declarative configuration schema data.
    pub const fn config_schema(&self) -> &ConfigSchema {
        &self.config_schema
    }

    /// Returns lifecycle capability metadata.
    pub const fn lifecycle(&self) -> LifecycleCapabilities {
        self.lifecycle
    }

    /// Creates the graph-local descriptor for one instance of this registered type.
    ///
    /// Registry metadata is type-level, while node and port ownership is graph-local.
    /// Rebinding keeps the registered port shape and replaces every embedded owner ID.
    pub fn for_node_id(&self, node_id: NodeId) -> Self {
        let ports = self
            .ports
            .iter()
            .map(|port| {
                PortDescriptor::new(
                    node_id.clone(),
                    port.name.clone(),
                    port.direction,
                    port.frame_type,
                )
            })
            .collect::<Vec<_>>();
        Self::new(
            node_id,
            self.node_type.clone(),
            self.kind,
            ports,
            self.config_schema.clone(),
            self.lifecycle,
        )
    }
}

/// Deterministically ordered node configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigMap(BTreeMap<ConfigKey, Value>);

impl ConfigMap {
    /// Creates an empty configuration.
    pub fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Creates a configuration and rejects duplicate keys.
    pub fn try_from_iter<I>(values: I) -> std::result::Result<Self, DuplicateConfigKey>
    where
        I: IntoIterator<Item = (ConfigKey, Value)>,
    {
        let mut map = BTreeMap::new();
        for (key, value) in values {
            if map.insert(key.clone(), value).is_some() {
                return Err(DuplicateConfigKey(key));
            }
        }
        Ok(Self(map))
    }

    /// Returns a configuration value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Iterates over configuration in stable key order.
    pub fn iter(&self) -> impl Iterator<Item = (&ConfigKey, &Value)> {
        self.0.iter()
    }

    /// Returns whether this map contains no values.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Default for ConfigMap {
    fn default() -> Self {
        Self::empty()
    }
}

impl std::borrow::Borrow<str> for ConfigKey {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

/// A duplicate key supplied while constructing a [`ConfigMap`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicateConfigKey(ConfigKey);

impl DuplicateConfigKey {
    /// Returns the duplicated key.
    pub fn key(&self) -> &ConfigKey {
        &self.0
    }
}

impl fmt::Display for DuplicateConfigKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "duplicate configuration key `{}`", self.0)
    }
}

impl Error for DuplicateConfigKey {}

/// One explicit output emission collected during a lifecycle call.
#[derive(Clone, Eq, PartialEq)]
pub struct NodeEmission {
    output_port: PortName,
    frame: Frame,
}

impl NodeEmission {
    /// Returns the explicit output port.
    pub fn output_port(&self) -> &PortName {
        &self.output_port
    }

    /// Returns the emitted immutable frame.
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// Splits the emission into its explicit port and frame pair.
    pub fn into_parts(self) -> (PortName, Frame) {
        (self.output_port, self.frame)
    }
}

/// Restricted per-call node context with no reference to a graph runner.
pub struct NodeContext {
    node_id: NodeId,
    config: ConfigMap,
    input_port: Option<PortName>,
    emissions: Vec<NodeEmission>,
    signals: Vec<SignalFrame>,
    emission_limit: usize,
    emission_overflowed: bool,
    has_signal_routes: bool,
    event_bus: EventBus,
    resources: ResourceStore,
    next_source_tick: Option<Duration>,
}

/// Returned when one lifecycle call exceeds its bounded emission budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeEmissionError {
    limit: usize,
}

impl NodeEmissionError {
    /// Returns the maximum number of emissions retained for the call.
    pub const fn limit(self) -> usize {
        self.limit
    }
}

impl fmt::Display for NodeEmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "node lifecycle call exceeded its emission limit of {}",
            self.limit
        )
    }
}

impl Error for NodeEmissionError {}

impl From<NodeEmissionError> for muxiva_types::MuxivaError {
    fn from(error: NodeEmissionError) -> Self {
        muxiva_types::MuxivaError::new(
            muxiva_types::ErrorCategory::Validation,
            "MUXIVA-NODE-EMISSION-LIMIT",
            error.to_string(),
        )
    }
}

/// Structured rejection from [`NodeContext::emit_signal`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignalEmissionError {
    NoConnectedDownstream {
        node_id: NodeId,
    },
    SourceMismatch {
        context_node: NodeId,
        signal_source: NodeId,
    },
    Limit {
        limit: usize,
    },
}

impl fmt::Display for SignalEmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConnectedDownstream { node_id } => {
                write!(
                    formatter,
                    "node `{node_id}` has no connected downstream edge"
                )
            }
            Self::SourceMismatch {
                context_node,
                signal_source,
            } => write!(
                formatter,
                "signal source `{signal_source}` does not match emitting node `{context_node}`"
            ),
            Self::Limit { limit } => write!(
                formatter,
                "node lifecycle call exceeded its signal emission limit of {limit}"
            ),
        }
    }
}

impl Error for SignalEmissionError {}

impl From<SignalEmissionError> for muxiva_types::MuxivaError {
    fn from(error: SignalEmissionError) -> Self {
        let code = match &error {
            SignalEmissionError::NoConnectedDownstream { .. } => "MUXIVA-SIGNAL-NO-EDGE",
            SignalEmissionError::SourceMismatch { .. } => "MUXIVA-SIGNAL-SOURCE",
            SignalEmissionError::Limit { .. } => "MUXIVA-SIGNAL-LIMIT",
        };
        muxiva_types::MuxivaError::new(
            muxiva_types::ErrorCategory::Validation,
            code,
            error.to_string(),
        )
    }
}

impl NodeContext {
    /// Creates a context for one node call.
    pub fn new(node_id: NodeId, config: ConfigMap, input_port: Option<PortName>) -> Self {
        Self::with_emission_limit(node_id, config, input_port, 16_384)
    }

    /// Creates a context with an explicit per-call emission allocation limit.
    pub fn with_emission_limit(
        node_id: NodeId,
        config: ConfigMap,
        input_port: Option<PortName>,
        emission_limit: usize,
    ) -> Self {
        Self::with_routing_limits(
            node_id,
            config,
            input_port,
            emission_limit,
            false,
            EventBus::default(),
            ResourceStore::new(),
        )
    }

    pub(crate) fn with_routing_limits(
        node_id: NodeId,
        config: ConfigMap,
        input_port: Option<PortName>,
        emission_limit: usize,
        has_signal_routes: bool,
        event_bus: EventBus,
        resources: ResourceStore,
    ) -> Self {
        Self {
            node_id,
            config,
            input_port,
            emissions: Vec::new(),
            signals: Vec::new(),
            emission_limit,
            emission_overflowed: false,
            has_signal_routes,
            event_bus,
            resources,
            next_source_tick: None,
        }
    }

    pub(crate) fn with_runtime_services(
        node_id: NodeId,
        config: ConfigMap,
        input_port: Option<PortName>,
        event_bus: EventBus,
        resources: ResourceStore,
    ) -> Self {
        Self::with_routing_limits(
            node_id, config, input_port, 16_384, false, event_bus, resources,
        )
    }

    /// Returns the node currently being called.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns immutable node configuration.
    pub const fn config(&self) -> &ConfigMap {
        &self.config
    }

    /// Returns the graph-local typed resources available to this runtime.
    ///
    /// Secrets and Node clients belong here rather than in serializable
    /// node configuration.
    pub const fn resources(&self) -> &ResourceStore {
        &self.resources
    }

    /// Returns the explicit input port for an edge delivery.
    ///
    /// This is `None` for lifecycle calls and source invocation.
    pub fn input_port(&self) -> Option<&PortName> {
        self.input_port.as_ref()
    }

    /// Collects a frame for one explicit output port.
    pub fn emit(
        &mut self,
        output_port: PortName,
        frame: Frame,
    ) -> std::result::Result<(), NodeEmissionError> {
        if self.emissions.len() >= self.emission_limit {
            self.emission_overflowed = true;
            return Err(NodeEmissionError {
                limit: self.emission_limit,
            });
        }
        self.emissions.push(NodeEmission { output_port, frame });
        Ok(())
    }

    /// Returns emissions collected so far.
    pub fn emissions(&self) -> &[NodeEmission] {
        &self.emissions
    }

    /// Drains emissions in call order.
    pub fn take_emissions(&mut self) -> Vec<NodeEmission> {
        std::mem::take(&mut self.emissions)
    }

    /// Queues a control frame for every actually connected downstream edge.
    pub fn emit_signal(
        &mut self,
        signal: SignalFrame,
    ) -> std::result::Result<(), SignalEmissionError> {
        if !self.has_signal_routes {
            return Err(SignalEmissionError::NoConnectedDownstream {
                node_id: self.node_id.clone(),
            });
        }
        if signal.data().source() != &self.node_id {
            return Err(SignalEmissionError::SourceMismatch {
                context_node: self.node_id.clone(),
                signal_source: signal.data().source().clone(),
            });
        }
        if self.signals.len() >= self.emission_limit {
            self.emission_overflowed = true;
            return Err(SignalEmissionError::Limit {
                limit: self.emission_limit,
            });
        }
        self.signals.push(signal);
        Ok(())
    }

    pub fn signals(&self) -> &[SignalFrame] {
        &self.signals
    }

    pub fn take_signals(&mut self) -> Vec<SignalFrame> {
        std::mem::take(&mut self.signals)
    }

    /// Returns the runtime-wide low-frequency EventBus.
    pub const fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Publishes a low-frequency global event without using a graph output port.
    pub fn publish_event(
        &self,
        event: EventFrame,
    ) -> std::result::Result<PublishReport, muxiva_types::MuxivaError> {
        self.event_bus.publish(event).map_err(|error| {
            muxiva_types::MuxivaError::new(
                muxiva_types::ErrorCategory::Internal,
                "MUXIVA-EVENTBUS-PUBLISH",
                error.to_string(),
            )
        })
    }

    /// Requests another source callback after `delay`.
    ///
    /// Only source workers honor this request. Omitting it completes the source,
    /// which preserves the one-shot source behavior used by existing nodes.
    pub fn schedule_next_tick(&mut self, delay: Duration) {
        self.next_source_tick = Some(delay);
    }

    pub(crate) fn take_next_source_tick(&mut self) -> Option<Duration> {
        self.next_source_tick.take()
    }

    pub(crate) const fn emission_overflowed(&self) -> bool {
        self.emission_overflowed
    }
}

/// The lifecycle phase in which graph execution aborted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AbortStage {
    /// Preparing node resources.
    Prepare,
    /// Processing a source tick or delivered frame.
    Process,
    /// Finishing prepared node resources.
    Finish,
    /// Runtime work outside a lifecycle callback.
    Runtime,
}

/// Stable categories that explain why execution aborted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AbortCategory {
    /// User or runtime cancellation.
    Cancelled,
    /// A node returned an ordinary error.
    NodeError,
    /// Rust code panicked at a protected task boundary.
    RustPanic,
    /// A foreign-language node raised or rejected an exception.
    ForeignException,
    /// An external native SDK reported a failure.
    ExternalSdkError,
}

/// Root failure information safe to retain and pass to `on_abort`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbortRootContext {
    code: Box<str>,
    message: Box<str>,
    details: ConfigMap,
}

impl AbortRootContext {
    /// Creates root context. The code and message are stable owned data.
    pub fn new(
        code: impl Into<Box<str>>,
        message: impl Into<Box<str>>,
        details: ConfigMap,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details,
        }
    }

    /// Returns the stable root code.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the root message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns deterministic structured details.
    pub const fn details(&self) -> &ConfigMap {
        &self.details
    }
}

/// Unified abort information delivered to prepared nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AbortReason {
    category: AbortCategory,
    node_id: Option<NodeId>,
    stage: AbortStage,
    root: AbortRootContext,
}

impl AbortReason {
    /// Creates a unified abort reason.
    pub fn new(
        category: AbortCategory,
        node_id: Option<NodeId>,
        stage: AbortStage,
        root: AbortRootContext,
    ) -> Self {
        Self {
            category,
            node_id,
            stage,
            root,
        }
    }

    /// Returns the stable abort category.
    pub const fn category(&self) -> AbortCategory {
        self.category
    }

    /// Returns the failing node when one is known.
    pub fn node_id(&self) -> Option<&NodeId> {
        self.node_id.as_ref()
    }

    /// Returns the stage in which the root failure occurred.
    pub const fn stage(&self) -> AbortStage {
        self.stage
    }

    /// Returns owned root failure context.
    pub const fn root(&self) -> &AbortRootContext {
        &self.root
    }
}

/// A Muxiva processing node with exactly the four uniform lifecycle hooks.
pub trait Node: Send {
    /// Prepares resources before frame processing begins.
    fn on_prepare(&mut self, _context: &mut NodeContext) -> Result<()> {
        Ok(())
    }

    /// Processes the only graph data type.
    ///
    /// A future synchronous runner passes `None` exactly for its one source
    /// invocation. Edge deliveries always pass `Some(Frame)`.
    fn on_process(&mut self, input: Option<Frame>, context: &mut NodeContext) -> Result<()>;

    /// Receives a graph-local control frame on the node's worker domain.
    /// This callback is not a fifth lifecycle hook.
    fn on_signal(&mut self, _signal: SignalFrame, _context: &mut NodeContext) -> Result<()> {
        Ok(())
    }

    /// Completes normal resource handling.
    fn on_finish(&mut self, _context: &mut NodeContext) -> Result<()> {
        Ok(())
    }

    /// Releases resources after failure or cancellation.
    fn on_abort(&mut self, _reason: &AbortReason, _context: &mut NodeContext) {}
}
