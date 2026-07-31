#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    error::Error,
    sync::{Arc, Mutex},
};

use voxa_core::{
    ConfigSchema, EdgeDescriptor, EnabledCondition, GraphBuilder, GraphRunner,
    LifecycleCapabilities, Node, NodeContext, NodeDescriptor, NodeInstances, NodeKind,
    NodeTypeName, PortDescriptor, PortDirection, PortName, QueuePolicy, TransformPolicy,
    ValidationPolicy, VisibilityDescriptor,
};
use voxa_types::{
    ClockDomain, ClockDomainId, ClockKind, EdgeId, Extensions, Frame, FrameHeader, FrameId,
    FramePayload, FrameType, Lineage, Metadata, NodeId, SequenceId, StreamId, TextData, Timestamp,
    TraceId,
};

const SOURCE_ID: &str = "text-source";
const TRANSFORM_ID: &str = "uppercase-transform";
const SINK_ID: &str = "collect-sink";
const SOURCE_PORT: &str = "text_out";
const TRANSFORM_INPUT_PORT: &str = "text_in";
const TRANSFORM_OUTPUT_PORT: &str = "text_out";
const SINK_PORT: &str = "text_in";

/// Emits a finite, deterministic sequence of text frames.
struct TextSource {
    frames: Vec<Frame>,
}

impl Node for TextSource {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        debug_assert!(input.is_none(), "sources are invoked once with None");
        for frame in &self.frames {
            context.emit(port(SOURCE_PORT), frame.clone())?;
        }
        Ok(())
    }
}

/// Uppercases each incoming text frame and emits a fresh text frame.
struct UppercaseTransform {
    next_sequence: u64,
}

impl Node for UppercaseTransform {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.expect("a Transform receives an Edge-delivered frame");
        let text = input
            .as_text()
            .expect("the explicit input port accepts only Text frames")
            .data()
            .as_str()
            .to_uppercase();
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        context.emit(
            port(TRANSFORM_OUTPUT_PORT),
            text_frame(&format!("uppercase-{sequence}"), &text, sequence),
        )?;
        Ok(())
    }
}

/// Collects transformed strings for deterministic reporting after the run.
struct CollectSink {
    collected: Arc<Mutex<Vec<String>>>,
}

impl Node for CollectSink {
    fn on_process(
        &mut self,
        input: Option<Frame>,
        _context: &mut NodeContext,
    ) -> voxa_types::Result<()> {
        let input = input.expect("a Sink receives an Edge-delivered frame");
        self.collected.lock().unwrap().push(
            input
                .as_text()
                .expect("the explicit input port accepts Text")
                .data()
                .as_str()
                .into(),
        );
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let graph = text_graph();
    let collected = Arc::new(Mutex::new(Vec::new()));
    let instances: NodeInstances = BTreeMap::from([
        (
            node_id(SOURCE_ID),
            Box::new(TextSource {
                frames: vec![
                    text_frame("source-1", "hello", 1),
                    text_frame("source-2", "voxa", 2),
                ],
            }) as Box<dyn Node>,
        ),
        (
            node_id(TRANSFORM_ID),
            Box::new(UppercaseTransform { next_sequence: 1 }) as Box<dyn Node>,
        ),
        (
            node_id(SINK_ID),
            Box::new(CollectSink {
                collected: collected.clone(),
            }) as Box<dyn Node>,
        ),
    ]);

    let mut runner = GraphRunner::new(&graph, instances, BTreeMap::new())?;
    runner.run().map_err(|reason| {
        std::io::Error::other(format!(
            "the text graph aborted: {} {}",
            reason.root().code(),
            reason.root().message()
        ))
    })?;

    println!(
        "Collected uppercase text: {}",
        collected.lock().unwrap().join(", ")
    );
    Ok(())
}

fn text_graph() -> voxa_core::GraphDefinition {
    let source = node_descriptor(
        SOURCE_ID,
        "example.text_source",
        NodeKind::Source,
        [(SOURCE_PORT, PortDirection::Output)],
    );
    let transform = node_descriptor(
        TRANSFORM_ID,
        "example.uppercase_transform",
        NodeKind::Transform,
        [
            (TRANSFORM_INPUT_PORT, PortDirection::Input),
            (TRANSFORM_OUTPUT_PORT, PortDirection::Output),
        ],
    );
    let sink = node_descriptor(
        SINK_ID,
        "example.collect_sink",
        NodeKind::Sink,
        [(SINK_PORT, PortDirection::Input)],
    );

    let mut builder = GraphBuilder::new();
    builder
        .add_node(source)
        .expect("the source descriptor is valid")
        .add_node(transform)
        .expect("the transform descriptor is valid")
        .add_node(sink)
        .expect("the sink descriptor is valid")
        .connect(edge(
            "source-to-uppercase",
            SOURCE_ID,
            SOURCE_PORT,
            TRANSFORM_ID,
            TRANSFORM_INPUT_PORT,
        ))
        .expect("the source-to-transform ports are explicit and compatible")
        .connect(edge(
            "uppercase-to-sink",
            TRANSFORM_ID,
            TRANSFORM_OUTPUT_PORT,
            SINK_ID,
            SINK_PORT,
        ))
        .expect("the transform-to-sink ports are explicit and compatible");
    builder.build().expect("the text graph is acyclic")
}

fn node_descriptor<const N: usize>(
    id: &str,
    node_type: &str,
    kind: NodeKind,
    ports: [(&str, PortDirection); N],
) -> NodeDescriptor {
    let node_id = node_id(id);
    NodeDescriptor::new(
        node_id.clone(),
        NodeTypeName::new(node_type).expect("the example node type is valid"),
        kind,
        ports.map(|(name, direction)| {
            PortDescriptor::new(node_id.clone(), port(name), direction, FrameType::Text)
        }),
        ConfigSchema::empty(),
        LifecycleCapabilities::default(),
    )
}

fn edge(id: &str, from: &str, output: &str, to: &str, input: &str) -> EdgeDescriptor {
    EdgeDescriptor::new(
        EdgeId::new(id).expect("the example Edge ID is valid"),
        node_id(from),
        port(output),
        node_id(to),
        port(input),
        FrameType::Text,
        QueuePolicy::default(),
        ValidationPolicy::TypeGateOnly,
        TransformPolicy::Identity,
        EnabledCondition::Always,
        VisibilityDescriptor::default(),
    )
}

fn text_frame(id: &str, text: &str, sequence: u64) -> Frame {
    Frame::new(
        FrameHeader::new(
            FrameId::new(id).expect("the example frame ID is valid"),
            Timestamp::from_nanos(
                i64::try_from(sequence.saturating_mul(1_000_000))
                    .expect("the example timestamp fits in i64"),
            ),
            ClockDomain::new(
                ClockDomainId::new("example.text").expect("the example clock domain is valid"),
                ClockKind::MediaRelative,
            ),
            SequenceId::new(sequence),
            StreamId::new("example-stream").expect("the example stream ID is valid"),
            TraceId::new("example-trace").expect("the example trace ID is valid"),
            FrameType::Text,
            Metadata::empty(),
            Extensions::empty(),
            Lineage::empty(),
        )
        .expect("the example text header is valid"),
        FramePayload::Text(TextData::new(text)),
    )
    .expect("the example text frame is valid")
}

fn node_id(value: &str) -> NodeId {
    NodeId::new(value).expect("the example node ID is valid")
}

fn port(value: &str) -> PortName {
    PortName::new(value).expect("the example port name is valid")
}
