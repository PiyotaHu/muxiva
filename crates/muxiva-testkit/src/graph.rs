use muxiva_core::{
    ConfigSchema, EdgeDescriptor, EnabledCondition, GraphBuildError, GraphBuilder, GraphDefinition,
    LifecycleCapabilities, NodeDescriptor, NodeKind, NodeTypeName, PortDescriptor, PortDirection,
    PortName, QueueOverflowPolicy, QueuePolicy, TransformPolicy, ValidationPolicy,
    VisibilityDescriptor,
};
use muxiva_types::{EdgeId, FrameType, GraphId, NodeId};
use std::num::NonZeroUsize;
pub struct TestGraphBuilder(GraphBuilder);
impl TestGraphBuilder {
    pub fn new(id: &str) -> Self {
        Self(GraphBuilder::with_graph_id(GraphId::new(id).unwrap()))
    }
    pub fn text_node(&mut self, id: &str, kind: NodeKind) -> Result<&mut Self, GraphBuildError> {
        let node = NodeId::new(id).unwrap();
        let mut ports = Vec::new();
        if kind != NodeKind::Source {
            ports.push(PortDescriptor::new(
                node.clone(),
                PortName::new("in").unwrap(),
                PortDirection::Input,
                FrameType::Text,
            ))
        }
        if kind != NodeKind::Sink {
            ports.push(PortDescriptor::new(
                node.clone(),
                PortName::new("out").unwrap(),
                PortDirection::Output,
                FrameType::Text,
            ))
        }
        self.0.add_node(NodeDescriptor::new(
            node,
            NodeTypeName::new("test.text").unwrap(),
            kind,
            ports,
            ConfigSchema::empty(),
            LifecycleCapabilities::default(),
        ))?;
        Ok(self)
    }
    pub fn connect_text(
        &mut self,
        id: &str,
        from: &str,
        to: &str,
        capacity: usize,
    ) -> Result<&mut Self, GraphBuildError> {
        self.0.connect(EdgeDescriptor::new(
            EdgeId::new(id).unwrap(),
            NodeId::new(from).unwrap(),
            PortName::new("out").unwrap(),
            NodeId::new(to).unwrap(),
            PortName::new("in").unwrap(),
            FrameType::Text,
            QueuePolicy::new(
                NonZeroUsize::new(capacity).unwrap(),
                QueueOverflowPolicy::Block,
            ),
            ValidationPolicy::TypeGateOnly,
            TransformPolicy::Identity,
            EnabledCondition::Always,
            VisibilityDescriptor::default(),
        ))?;
        Ok(self)
    }
    pub fn build(self) -> Result<GraphDefinition, GraphBuildError> {
        self.0.build()
    }
}
