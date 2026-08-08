use muxiva_core::{
    ConfigKey, ConfigMap, ConfigSchema, EdgeDescriptor, EnabledCondition, GraphBuildError,
    GraphBuilder, LifecycleCapabilities, NodeDescriptor, NodeKind, NodeTypeName, PortDescriptor,
    PortDirection, PortName, QueuePolicy, TransformPolicy, ValidationPolicy, VisibilityDescriptor,
};
use muxiva_types::{EdgeId, FrameType, NodeId, Value};

fn node_id(value: &str) -> NodeId {
    NodeId::new(value).unwrap()
}

fn port_name(value: &str) -> PortName {
    PortName::new(value).unwrap()
}

fn node(id: &str, kind: NodeKind, ports: &[(&str, PortDirection, FrameType)]) -> NodeDescriptor {
    let id = node_id(id);
    let ports = ports
        .iter()
        .map(|(name, direction, frame_type)| {
            PortDescriptor::new(id.clone(), port_name(name), *direction, *frame_type)
        })
        .collect::<Vec<_>>();
    NodeDescriptor::new(
        id,
        NodeTypeName::new(format!("test.{kind:?}")).unwrap(),
        kind,
        ports,
        ConfigSchema::empty(),
        LifecycleCapabilities::default(),
    )
}

fn edge(
    id: &str,
    source_node: &str,
    source_port: &str,
    target_node: &str,
    target_port: &str,
    frame_type: FrameType,
) -> EdgeDescriptor {
    EdgeDescriptor::new(
        EdgeId::new(id).unwrap(),
        node_id(source_node),
        port_name(source_port),
        node_id(target_node),
        port_name(target_port),
        frame_type,
        QueuePolicy::default(),
        ValidationPolicy::TypeGateOnly,
        TransformPolicy::Identity,
        EnabledCondition::Always,
        VisibilityDescriptor::default(),
    )
}

#[test]
fn empty_graph_is_valid_and_has_empty_topology() {
    let graph = GraphBuilder::new().build().unwrap();

    assert!(graph.is_empty());
    assert!(graph.nodes().is_empty());
    assert!(graph.edges().is_empty());
    assert!(graph.topological_order().is_empty());
}

#[test]
fn duplicate_node_and_edge_ids_are_rejected() {
    let source = node(
        "source",
        NodeKind::Source,
        &[("out", PortDirection::Output, FrameType::Text)],
    );
    let sink = node(
        "sink",
        NodeKind::Sink,
        &[("in", PortDirection::Input, FrameType::Text)],
    );
    let mut duplicate_nodes = GraphBuilder::new();
    duplicate_nodes.add_node(source.clone()).unwrap();
    assert!(matches!(
        duplicate_nodes.add_node(source),
        Err(GraphBuildError::DuplicateNode { node_id }) if node_id.as_str() == "source"
    ));

    let mut duplicate_edges = GraphBuilder::new();
    duplicate_edges
        .add_node(node(
            "source",
            NodeKind::Source,
            &[("out", PortDirection::Output, FrameType::Text)],
        ))
        .unwrap()
        .add_node(sink)
        .unwrap();
    duplicate_edges
        .connect(edge(
            "edge-1",
            "source",
            "out",
            "sink",
            "in",
            FrameType::Text,
        ))
        .unwrap();
    assert!(matches!(
        duplicate_edges.connect(edge(
            "edge-1",
            "source",
            "out",
            "sink",
            "in",
            FrameType::Text,
        )),
        Err(GraphBuildError::DuplicateEdge { edge_id, .. }) if edge_id.as_str() == "edge-1"
    ));
}

#[test]
fn missing_ports_and_wrong_directions_include_explicit_endpoint_context() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(node(
            "source",
            NodeKind::Source,
            &[("text_out", PortDirection::Output, FrameType::Text)],
        ))
        .unwrap()
        .add_node(node(
            "transform",
            NodeKind::Transform,
            &[
                ("text_in", PortDirection::Input, FrameType::Text),
                ("text_out", PortDirection::Output, FrameType::Text),
            ],
        ))
        .unwrap();

    let error = builder
        .connect(edge(
            "missing-port",
            "source",
            "absent",
            "transform",
            "text_in",
            FrameType::Text,
        ))
        .unwrap_err();
    assert!(matches!(
        &error,
        GraphBuildError::MissingPort {
            edge_id,
            source,
            target,
            ..
        } if edge_id.as_str() == "missing-port"
            && source.to_string() == "source.absent"
            && target.to_string() == "transform.text_in"
    ));
    assert!(error.to_string().contains("source.absent"));

    let error = builder
        .connect(edge(
            "wrong-direction",
            "transform",
            "text_in",
            "transform",
            "text_in",
            FrameType::Text,
        ))
        .unwrap_err();
    assert!(matches!(
        error,
        GraphBuildError::DirectionMismatch {
            expected: PortDirection::Output,
            actual: PortDirection::Input,
            ..
        }
    ));
}

#[test]
fn exact_audio_to_video_mismatch_requires_an_explicit_transform() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(node(
            "audio-source",
            NodeKind::Source,
            &[("audio_out", PortDirection::Output, FrameType::Audio)],
        ))
        .unwrap()
        .add_node(node(
            "video-sink",
            NodeKind::Sink,
            &[("video_in", PortDirection::Input, FrameType::Video)],
        ))
        .unwrap();

    let error = builder
        .connect(edge(
            "media-edge",
            "audio-source",
            "audio_out",
            "video-sink",
            "video_in",
            FrameType::Audio,
        ))
        .unwrap_err();

    match error {
        GraphBuildError::TypeMismatch {
            edge_id,
            source,
            target,
            source_type,
            target_type,
            edge_type,
            suggested_explicit_transform,
        } => {
            assert_eq!(edge_id.as_str(), "media-edge");
            assert_eq!(source.to_string(), "audio-source.audio_out");
            assert_eq!(target.to_string(), "video-sink.video_in");
            assert_eq!(source_type, FrameType::Audio);
            assert_eq!(target_type, FrameType::Video);
            assert_eq!(edge_type, FrameType::Audio);
            assert!(suggested_explicit_transform.contains("TransformNode"));
            assert!(suggested_explicit_transform.contains("Audio"));
            assert!(suggested_explicit_transform.contains("Video"));
        }
        other => panic!("expected type mismatch, got {other:?}"),
    }
}

#[test]
fn cycle_detection_reports_stable_participating_node_ids() {
    let mut builder = GraphBuilder::new();
    for id in ["node-b", "node-a"] {
        builder
            .add_node(node(
                id,
                NodeKind::Transform,
                &[
                    ("in", PortDirection::Input, FrameType::Text),
                    ("out", PortDirection::Output, FrameType::Text),
                ],
            ))
            .unwrap();
    }
    builder
        .connect(edge(
            "a-to-b",
            "node-a",
            "out",
            "node-b",
            "in",
            FrameType::Text,
        ))
        .unwrap()
        .connect(edge(
            "b-to-a",
            "node-b",
            "out",
            "node-a",
            "in",
            FrameType::Text,
        ))
        .unwrap();

    assert!(matches!(
        builder.build(),
        Err(GraphBuildError::Cycle { node_ids })
            if node_ids.iter().map(NodeId::as_str).collect::<Vec<_>>() == ["node-a", "node-b"]
    ));
}

#[test]
fn topological_order_and_storage_are_deterministic_across_insertion_order() {
    fn build(reverse: bool) -> muxiva_core::GraphDefinition {
        let descriptors = [
            node(
                "source-b",
                NodeKind::Source,
                &[("out", PortDirection::Output, FrameType::Text)],
            ),
            node(
                "sink-z",
                NodeKind::Sink,
                &[("in", PortDirection::Input, FrameType::Text)],
            ),
            node(
                "source-a",
                NodeKind::Source,
                &[("out", PortDirection::Output, FrameType::Text)],
            ),
        ];
        let edges = [
            edge("edge-b", "source-b", "out", "sink-z", "in", FrameType::Text),
            edge("edge-a", "source-a", "out", "sink-z", "in", FrameType::Text),
        ];
        let mut builder = GraphBuilder::new();
        if reverse {
            for descriptor in descriptors.into_iter().rev() {
                builder.add_node(descriptor).unwrap();
            }
            for descriptor in edges.into_iter().rev() {
                builder.connect(descriptor).unwrap();
            }
        } else {
            for descriptor in descriptors {
                builder.add_node(descriptor).unwrap();
            }
            for descriptor in edges {
                builder.connect(descriptor).unwrap();
            }
        }
        builder.build().unwrap()
    }

    let first = build(false);
    let second = build(true);

    assert_eq!(first, second);
    assert_eq!(
        first
            .topological_order()
            .iter()
            .map(NodeId::as_str)
            .collect::<Vec<_>>(),
        ["source-a", "source-b", "sink-z"]
    );
    assert_eq!(
        first
            .edges()
            .iter()
            .map(|edge| edge.edge_id().as_str())
            .collect::<Vec<_>>(),
        ["edge-a", "edge-b"]
    );
}

#[test]
fn stable_node_edge_port_and_config_ids_survive_build() {
    let mut builder = GraphBuilder::new();
    builder
        .add_node(node(
            "source-id",
            NodeKind::Source,
            &[("stable-output", PortDirection::Output, FrameType::Text)],
        ))
        .unwrap()
        .add_node(node(
            "sink-id",
            NodeKind::Sink,
            &[("stable-input", PortDirection::Input, FrameType::Text)],
        ))
        .unwrap()
        .set_config(
            &node_id("source-id"),
            ConfigMap::try_from_iter([(
                ConfigKey::new("message").unwrap(),
                Value::String(Box::from("hello")),
            )])
            .unwrap(),
        )
        .unwrap()
        .connect(edge(
            "stable-edge-id",
            "source-id",
            "stable-output",
            "sink-id",
            "stable-input",
            FrameType::Text,
        ))
        .unwrap();

    let graph = builder.build().unwrap();
    let source = graph.node(&node_id("source-id")).unwrap();
    let stored_edge = graph.edge(&EdgeId::new("stable-edge-id").unwrap()).unwrap();

    assert_eq!(source.descriptor().node_id().as_str(), "source-id");
    assert_eq!(
        source.descriptor().ports()[0].name().as_str(),
        "stable-output"
    );
    assert_eq!(
        source.config().get("message"),
        Some(&Value::String(Box::from("hello")))
    );
    assert_eq!(stored_edge.edge_id().as_str(), "stable-edge-id");
    assert_eq!(stored_edge.from_output_port().as_str(), "stable-output");
    assert_eq!(stored_edge.to_input_port().as_str(), "stable-input");
}
