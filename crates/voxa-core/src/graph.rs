use std::{collections::BTreeMap, collections::BTreeSet, error::Error, fmt, fmt::Write};

use voxa_types::{EdgeId, FrameType, GraphId, NodeId};

use crate::{
    edge::{EdgeDescriptor, QueueOverflowPolicy},
    node::{ConfigMap, NodeDescriptor, NodeKind, PortDirection, PortName},
    NodeFactorySelection,
};

/// One fully explicit endpoint used in graph validation diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeEndpoint {
    node_id: NodeId,
    port_name: PortName,
}

impl EdgeEndpoint {
    fn new(node_id: NodeId, port_name: PortName) -> Self {
        Self { node_id, port_name }
    }

    /// Returns the endpoint node ID.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Returns the endpoint port name.
    pub fn port_name(&self) -> &PortName {
        &self.port_name
    }
}

impl fmt::Display for EdgeEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.node_id, self.port_name)
    }
}

/// Whether an invalid endpoint is the source or target of an Edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndpointRole {
    /// The `from` endpoint.
    Source,
    /// The `to` endpoint.
    Target,
}

/// Structured errors returned by [`GraphBuilder`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphBuildError {
    /// Two node descriptors use the same stable ID.
    DuplicateNode { node_id: NodeId },
    /// Two Edge descriptors use the same stable ID.
    DuplicateEdge {
        edge_id: EdgeId,
        source: EdgeEndpoint,
        target: EdgeEndpoint,
    },
    /// A descriptor port claims a different owner.
    PortNodeMismatch {
        node_id: NodeId,
        port_node_id: NodeId,
        port_name: PortName,
    },
    /// A descriptor repeats a port in one direction.
    DuplicatePort {
        node_id: NodeId,
        port_name: PortName,
        direction: PortDirection,
    },
    /// The node kind conflicts with its port shape.
    InvalidNodeKind {
        node_id: NodeId,
        kind: NodeKind,
        message: Box<str>,
    },
    /// Lifecycle metadata says the mandatory process hook is unavailable.
    ProcessCapabilityMissing { node_id: NodeId },
    /// An explicit Edge endpoint names a missing node.
    MissingNode {
        edge_id: EdgeId,
        role: EndpointRole,
        missing_node_id: NodeId,
        source: EdgeEndpoint,
        target: EdgeEndpoint,
    },
    /// An explicit Edge endpoint names a missing port.
    MissingPort {
        edge_id: EdgeId,
        role: EndpointRole,
        endpoint: EdgeEndpoint,
        expected_direction: PortDirection,
        source: EdgeEndpoint,
        target: EdgeEndpoint,
    },
    /// The named port exists but points the wrong way.
    DirectionMismatch {
        edge_id: EdgeId,
        role: EndpointRole,
        endpoint: EdgeEndpoint,
        expected: PortDirection,
        actual: PortDirection,
        source: EdgeEndpoint,
        target: EdgeEndpoint,
    },
    /// Source, target, and Edge-declared exact types are not identical.
    TypeMismatch {
        edge_id: EdgeId,
        source: EdgeEndpoint,
        target: EdgeEndpoint,
        source_type: FrameType,
        target_type: FrameType,
        edge_type: FrameType,
        suggested_explicit_transform: Box<str>,
    },
    /// Configuration was supplied for a missing node.
    ConfigNodeMissing { node_id: NodeId },
    /// A Factory selection was supplied for a missing node.
    FactoryNodeMissing { node_id: NodeId },
    /// The complete graph contains a directed cycle.
    Cycle { node_ids: Box<[NodeId]> },
}

impl fmt::Display for GraphBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode { node_id } => write!(formatter, "duplicate node `{node_id}`"),
            Self::DuplicateEdge {
                edge_id,
                source,
                target,
            } => write!(
                formatter,
                "duplicate edge `{edge_id}` connecting `{source}` to `{target}`"
            ),
            Self::PortNodeMismatch {
                node_id,
                port_node_id,
                port_name,
            } => write!(
                formatter,
                "node `{node_id}` contains port `{port_name}` owned by `{port_node_id}`"
            ),
            Self::DuplicatePort {
                node_id,
                port_name,
                direction,
            } => write!(
                formatter,
                "node `{node_id}` has duplicate {direction:?} port `{port_name}`"
            ),
            Self::InvalidNodeKind {
                node_id,
                kind,
                message,
            } => write!(formatter, "invalid {kind:?} node `{node_id}`: {message}"),
            Self::ProcessCapabilityMissing { node_id } => write!(
                formatter,
                "node `{node_id}` does not declare the mandatory process capability"
            ),
            Self::MissingNode {
                edge_id,
                role,
                missing_node_id,
                source,
                target,
            } => write!(
                formatter,
                "edge `{edge_id}` from `{source}` to `{target}` has missing {role:?} node `{missing_node_id}`"
            ),
            Self::MissingPort {
                edge_id,
                role,
                endpoint,
                expected_direction,
                source,
                target,
            } => write!(
                formatter,
                "edge `{edge_id}` from `{source}` to `{target}` has missing {role:?} {expected_direction:?} port `{endpoint}`"
            ),
            Self::DirectionMismatch {
                edge_id,
                role,
                endpoint,
                expected,
                actual,
                source,
                target,
            } => write!(
                formatter,
                "edge `{edge_id}` from `{source}` to `{target}` uses {role:?} port `{endpoint}` as {expected:?}, but it is {actual:?}"
            ),
            Self::TypeMismatch {
                edge_id,
                source,
                target,
                source_type,
                target_type,
                edge_type,
                suggested_explicit_transform,
            } => write!(
                formatter,
                "edge `{edge_id}` from `{source}` ({source_type:?}) to `{target}` ({target_type:?}) declares {edge_type:?}; {suggested_explicit_transform}"
            ),
            Self::ConfigNodeMissing { node_id } => {
                write!(formatter, "configuration targets missing node `{node_id}`")
            }
            Self::FactoryNodeMissing { node_id } => {
                write!(formatter, "Factory selection targets missing node `{node_id}`")
            }
            Self::Cycle { node_ids } => write!(
                formatter,
                "graph contains a directed cycle involving {}",
                node_ids
                    .iter()
                    .map(NodeId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl Error for GraphBuildError {}

/// A configured node in a stable graph definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeDefinition {
    descriptor: NodeDescriptor,
    config: ConfigMap,
    factory: Option<NodeFactorySelection>,
}

impl NodeDefinition {
    /// Returns node registration and port data.
    pub const fn descriptor(&self) -> &NodeDescriptor {
        &self.descriptor
    }

    /// Returns immutable configured values.
    pub const fn config(&self) -> &ConfigMap {
        &self.config
    }

    /// Returns the exact registered implementation selected by a compiled graph.
    /// Programmatic graphs that attach instances directly may leave this unset.
    pub const fn factory(&self) -> Option<&NodeFactorySelection> {
        self.factory.as_ref()
    }
}

/// Pure, stable graph data. It stores no node instances or policy callbacks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphDefinition {
    graph_id: GraphId,
    nodes: Box<[NodeDefinition]>,
    edges: Box<[EdgeDescriptor]>,
    topological_order: Box<[NodeId]>,
}

impl GraphDefinition {
    /// Returns the stable graph identity.
    pub const fn graph_id(&self) -> &GraphId {
        &self.graph_id
    }
    /// Returns nodes sorted by stable `NodeId`.
    pub fn nodes(&self) -> &[NodeDefinition] {
        &self.nodes
    }

    /// Returns Edges sorted by stable `EdgeId`.
    pub fn edges(&self) -> &[EdgeDescriptor] {
        &self.edges
    }

    /// Returns deterministic topological order, using `NodeId` to break ties.
    pub fn topological_order(&self) -> &[NodeId] {
        &self.topological_order
    }

    /// Returns one node definition by stable ID.
    pub fn node(&self, node_id: &NodeId) -> Option<&NodeDefinition> {
        self.nodes
            .binary_search_by(|node| node.descriptor.node_id().cmp(node_id))
            .ok()
            .map(|index| &self.nodes[index])
    }

    /// Returns one Edge descriptor by stable ID.
    pub fn edge(&self, edge_id: &EdgeId) -> Option<&EdgeDescriptor> {
        self.edges
            .binary_search_by(|edge| edge.edge_id().cmp(edge_id))
            .ok()
            .map(|index| &self.edges[index])
    }

    /// Returns whether this definition contains no nodes and no Edges.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Renders a deterministic, human-readable DSL for logs and developer tools.
    ///
    /// This presentation is intentionally not a machine-readable replacement
    /// for JSON Graph v1. It exposes node roles, typed ports, Edges, queue
    /// policies, and the actual branch/join structure without including
    /// payloads or secrets.
    pub fn render_human_dsl(&self) -> String {
        let mut output = String::new();
        writeln!(output, "graph \"{}\" {{", self.graph_id)
            .expect("writing to a String cannot fail");
        for node in &self.nodes {
            let descriptor = node.descriptor();
            writeln!(
                output,
                "  node \"{}\" kind={} type=\"{}\"",
                descriptor.node_id(),
                node_kind_name(descriptor.kind()),
                descriptor.node_type()
            )
            .expect("writing to a String cannot fail");
            for port in descriptor.ports() {
                writeln!(
                    output,
                    "    {} {}: {}",
                    port_direction_name(port.direction()),
                    port.name(),
                    frame_type_name(port.frame_type())
                )
                .expect("writing to a String cannot fail");
            }
        }
        for edge in &self.edges {
            let queue = edge.queue_policy();
            writeln!(
                output,
                "  edge \"{}\" {}.{} -> {}.{} frame={} queue={}/{}",
                edge.edge_id(),
                edge.from_node_id(),
                edge.from_output_port(),
                edge.to_node_id(),
                edge.to_input_port(),
                frame_type_name(edge.frame_type()),
                queue.capacity(),
                overflow_policy_name(queue.overflow())
            )
            .expect("writing to a String cannot fail");
        }
        output.push_str("}\n");
        output.push_str("flow:\n");
        for node_id in &self.topological_order {
            let outgoing = self
                .edges
                .iter()
                .filter(|edge| edge.from_node_id() == node_id)
                .collect::<Vec<_>>();
            if outgoing.is_empty() {
                continue;
            }
            writeln!(output, "  {node_id}").expect("writing to a String cannot fail");
            for (index, edge) in outgoing.iter().enumerate() {
                let connector = if index + 1 == outgoing.len() {
                    "└─"
                } else {
                    "├─"
                };
                writeln!(
                    output,
                    "    {connector}{}.{} [{}] -> {}.{}",
                    edge.from_node_id(),
                    edge.from_output_port(),
                    frame_type_name(edge.frame_type()),
                    edge.to_node_id(),
                    edge.to_input_port()
                )
                .expect("writing to a String cannot fail");
            }
        }
        output
    }
}

const fn node_kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Source => "source",
        NodeKind::Transform => "transform",
        NodeKind::Sink => "sink",
    }
}

const fn port_direction_name(direction: PortDirection) -> &'static str {
    match direction {
        PortDirection::Input => "input",
        PortDirection::Output => "output",
    }
}

const fn frame_type_name(kind: FrameType) -> &'static str {
    match kind {
        FrameType::Audio => "audio",
        FrameType::Video => "video",
        FrameType::Text => "text",
        FrameType::Byte => "byte",
        FrameType::Signal => "signal",
        FrameType::Event => "event",
    }
}

const fn overflow_policy_name(policy: QueueOverflowPolicy) -> &'static str {
    match policy {
        QueueOverflowPolicy::Block => "block",
        QueueOverflowPolicy::DropOldest => "drop_oldest",
        QueueOverflowPolicy::DropNewest => "drop_newest",
        QueueOverflowPolicy::Abort => "abort",
    }
}

/// Builds and validates a static directed acyclic graph without executing it.
#[derive(Debug)]
pub struct GraphBuilder {
    graph_id: GraphId,
    nodes: BTreeMap<NodeId, NodeDefinition>,
    edges: BTreeMap<EdgeId, EdgeDescriptor>,
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphBuilder {
    /// Creates an empty graph builder. Building it succeeds.
    pub fn new() -> Self {
        Self::with_graph_id(GraphId::new("graph").expect("constant graph identifier"))
    }

    /// Creates an empty graph builder with an explicit serializable identity.
    pub fn with_graph_id(graph_id: GraphId) -> Self {
        Self {
            graph_id,
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
        }
    }

    /// Adds one pure-data node descriptor.
    pub fn add_node(
        &mut self,
        descriptor: NodeDescriptor,
    ) -> std::result::Result<&mut Self, GraphBuildError> {
        validate_node_descriptor(&descriptor)?;
        let node_id = descriptor.node_id().clone();
        if self.nodes.contains_key(&node_id) {
            return Err(GraphBuildError::DuplicateNode { node_id });
        }
        self.nodes.insert(
            node_id,
            NodeDefinition {
                descriptor,
                config: ConfigMap::empty(),
                factory: None,
            },
        );
        Ok(self)
    }

    /// Connects two explicitly named, exactly typed ports.
    pub fn connect(
        &mut self,
        edge: EdgeDescriptor,
    ) -> std::result::Result<&mut Self, GraphBuildError> {
        let source = edge_source(&edge);
        let target = edge_target(&edge);
        if self.edges.contains_key(edge.edge_id()) {
            return Err(GraphBuildError::DuplicateEdge {
                edge_id: edge.edge_id().clone(),
                source,
                target,
            });
        }

        validate_edge(&self.nodes, &edge)?;
        self.edges.insert(edge.edge_id().clone(), edge);
        Ok(self)
    }

    /// Replaces the complete configuration for one existing node.
    pub fn set_config(
        &mut self,
        node_id: &NodeId,
        config: ConfigMap,
    ) -> std::result::Result<&mut Self, GraphBuildError> {
        let node =
            self.nodes
                .get_mut(node_id)
                .ok_or_else(|| GraphBuildError::ConfigNodeMissing {
                    node_id: node_id.clone(),
                })?;
        node.config = config;
        Ok(self)
    }

    /// Selects the exact executable Factory for one existing node.
    pub fn set_factory(
        &mut self,
        node_id: &NodeId,
        factory: NodeFactorySelection,
    ) -> std::result::Result<&mut Self, GraphBuildError> {
        let node =
            self.nodes
                .get_mut(node_id)
                .ok_or_else(|| GraphBuildError::FactoryNodeMissing {
                    node_id: node_id.clone(),
                })?;
        node.factory = Some(factory);
        Ok(self)
    }

    /// Validates acyclicity and returns pure stable graph data.
    pub fn build(self) -> std::result::Result<GraphDefinition, GraphBuildError> {
        let topological_order = stable_topological_order(&self.nodes, &self.edges)?;
        Ok(GraphDefinition {
            graph_id: self.graph_id,
            nodes: self.nodes.into_values().collect(),
            edges: self.edges.into_values().collect(),
            topological_order: topological_order.into_boxed_slice(),
        })
    }
}

fn validate_node_descriptor(descriptor: &NodeDescriptor) -> Result<(), GraphBuildError> {
    if !descriptor.lifecycle().process() {
        return Err(GraphBuildError::ProcessCapabilityMissing {
            node_id: descriptor.node_id().clone(),
        });
    }

    let mut ports = BTreeSet::new();
    for port in descriptor.ports() {
        if port.node_id() != descriptor.node_id() {
            return Err(GraphBuildError::PortNodeMismatch {
                node_id: descriptor.node_id().clone(),
                port_node_id: port.node_id().clone(),
                port_name: port.name().clone(),
            });
        }

        let key = (direction_order(port.direction()), port.name().clone());
        if !ports.insert(key) {
            return Err(GraphBuildError::DuplicatePort {
                node_id: descriptor.node_id().clone(),
                port_name: port.name().clone(),
                direction: port.direction(),
            });
        }

        if descriptor.kind() == NodeKind::Source && port.direction() == PortDirection::Input {
            return Err(GraphBuildError::InvalidNodeKind {
                node_id: descriptor.node_id().clone(),
                kind: descriptor.kind(),
                message: Box::from("Source nodes cannot declare input ports"),
            });
        }
        if descriptor.kind() == NodeKind::Sink && port.direction() == PortDirection::Output {
            return Err(GraphBuildError::InvalidNodeKind {
                node_id: descriptor.node_id().clone(),
                kind: descriptor.kind(),
                message: Box::from("Sink nodes cannot declare output ports"),
            });
        }
    }
    Ok(())
}

const fn direction_order(direction: PortDirection) -> u8 {
    match direction {
        PortDirection::Input => 0,
        PortDirection::Output => 1,
    }
}

fn validate_edge(
    nodes: &BTreeMap<NodeId, NodeDefinition>,
    edge: &EdgeDescriptor,
) -> Result<(), GraphBuildError> {
    let source = edge_source(edge);
    let target = edge_target(edge);
    let source_node =
        nodes
            .get(edge.from_node_id())
            .ok_or_else(|| GraphBuildError::MissingNode {
                edge_id: edge.edge_id().clone(),
                role: EndpointRole::Source,
                missing_node_id: edge.from_node_id().clone(),
                source: source.clone(),
                target: target.clone(),
            })?;
    let target_node = nodes
        .get(edge.to_node_id())
        .ok_or_else(|| GraphBuildError::MissingNode {
            edge_id: edge.edge_id().clone(),
            role: EndpointRole::Target,
            missing_node_id: edge.to_node_id().clone(),
            source: source.clone(),
            target: target.clone(),
        })?;

    let source_port = find_port(
        source_node,
        edge.from_output_port(),
        PortDirection::Output,
        edge,
        EndpointRole::Source,
    )?;
    let target_port = find_port(
        target_node,
        edge.to_input_port(),
        PortDirection::Input,
        edge,
        EndpointRole::Target,
    )?;

    if source_port.frame_type() != target_port.frame_type()
        || edge.frame_type() != source_port.frame_type()
        || edge.frame_type() != target_port.frame_type()
    {
        let suggested_explicit_transform = if source_port.frame_type() != target_port.frame_type() {
            format!(
                "insert an explicit TransformNode converting {:?} to {:?}",
                source_port.frame_type(),
                target_port.frame_type()
            )
        } else {
            format!(
                "no transform is needed; declare this Edge as exactly {:?}",
                source_port.frame_type()
            )
        };
        return Err(GraphBuildError::TypeMismatch {
            edge_id: edge.edge_id().clone(),
            source,
            target,
            source_type: source_port.frame_type(),
            target_type: target_port.frame_type(),
            edge_type: edge.frame_type(),
            suggested_explicit_transform: suggested_explicit_transform.into_boxed_str(),
        });
    }
    Ok(())
}

fn find_port<'a>(
    node: &'a NodeDefinition,
    name: &PortName,
    expected: PortDirection,
    edge: &EdgeDescriptor,
    role: EndpointRole,
) -> Result<&'a crate::node::PortDescriptor, GraphBuildError> {
    if let Some(port) = node
        .descriptor
        .ports()
        .iter()
        .find(|port| port.name() == name && port.direction() == expected)
    {
        return Ok(port);
    }

    let source = edge_source(edge);
    let target = edge_target(edge);
    let endpoint = match role {
        EndpointRole::Source => source.clone(),
        EndpointRole::Target => target.clone(),
    };
    if let Some(port) = node
        .descriptor
        .ports()
        .iter()
        .find(|port| port.name() == name)
    {
        return Err(GraphBuildError::DirectionMismatch {
            edge_id: edge.edge_id().clone(),
            role,
            endpoint,
            expected,
            actual: port.direction(),
            source,
            target,
        });
    }

    Err(GraphBuildError::MissingPort {
        edge_id: edge.edge_id().clone(),
        role,
        endpoint,
        expected_direction: expected,
        source,
        target,
    })
}

fn edge_source(edge: &EdgeDescriptor) -> EdgeEndpoint {
    EdgeEndpoint::new(edge.from_node_id().clone(), edge.from_output_port().clone())
}

fn edge_target(edge: &EdgeDescriptor) -> EdgeEndpoint {
    EdgeEndpoint::new(edge.to_node_id().clone(), edge.to_input_port().clone())
}

fn stable_topological_order(
    nodes: &BTreeMap<NodeId, NodeDefinition>,
    edges: &BTreeMap<EdgeId, EdgeDescriptor>,
) -> Result<Vec<NodeId>, GraphBuildError> {
    let mut indegree = nodes
        .keys()
        .cloned()
        .map(|node_id| (node_id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = nodes
        .keys()
        .cloned()
        .map(|node_id| (node_id, Vec::<NodeId>::new()))
        .collect::<BTreeMap<_, _>>();

    for edge in edges.values() {
        *indegree
            .get_mut(edge.to_node_id())
            .expect("validated Edge target must exist") += 1;
        outgoing
            .get_mut(edge.from_node_id())
            .expect("validated Edge source must exist")
            .push(edge.to_node_id().clone());
    }

    let mut ready = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(node_id, _)| node_id.clone())
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(nodes.len());

    while let Some(node_id) = ready.pop_first() {
        ordered.push(node_id.clone());
        for target in outgoing
            .get(&node_id)
            .expect("all nodes have an outgoing collection")
        {
            let degree = indegree
                .get_mut(target)
                .expect("validated Edge target must exist");
            *degree -= 1;
            if *degree == 0 {
                ready.insert(target.clone());
            }
        }
    }

    if ordered.len() != nodes.len() {
        let node_ids = indegree
            .into_iter()
            .filter(|(_, degree)| *degree != 0)
            .map(|(node_id, _)| node_id)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        return Err(GraphBuildError::Cycle { node_ids });
    }
    Ok(ordered)
}
