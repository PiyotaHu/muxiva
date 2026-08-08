use muxiva_core::{ManagedStreamAdapter, Node, NodeContext, NodeKind, PortName};
use muxiva_testkit::{
    audio_frame, text_frame, LifecycleCall, ManagedOutcome, ScriptedManagedStreamAdapter,
    TestGraphBuilder, TestNode, TestSink,
};
use std::sync::{Arc, Mutex};

#[test]
fn graph_and_node_helpers_record_owned_deterministic_data() {
    let graph = TestGraphBuilder::new("harness");
    let mut graph = graph;
    graph
        .text_node("source", NodeKind::Source)
        .unwrap()
        .text_node("sink", NodeKind::Sink)
        .unwrap()
        .connect_text("edge", "source", "sink", 2)
        .unwrap();
    assert_eq!(graph.build().unwrap().topological_order().len(), 2);
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut node = TestNode::new(log.clone()).emitting(PortName::new("out").unwrap());
    let mut context = NodeContext::new(
        muxiva_types::NodeId::new("node").unwrap(),
        muxiva_core::ConfigMap::empty(),
        None,
    );
    node.on_process(Some(text_frame(7, "hello")), &mut context)
        .unwrap();
    assert_eq!(*log.lock().unwrap(), vec![LifecycleCall::Process(7)]);
    assert_eq!(context.emissions().len(), 1);
}

#[test]
fn sink_frame_and_managed_helpers_are_copy_owned_and_scripted() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let mut sink = TestSink::new(captured.clone());
    let mut context = NodeContext::new(
        muxiva_types::NodeId::new("sink").unwrap(),
        muxiva_core::ConfigMap::empty(),
        None,
    );
    sink.on_process(Some(audio_frame(3)), &mut context).unwrap();
    assert_eq!(captured.lock().unwrap()[0].header().sequence_id().get(), 3);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let adapter =
        ScriptedManagedStreamAdapter::new(vec![ManagedOutcome::Failed("fault".into())], requests);
    let request = muxiva_core::AdapterRequest {
        request_id: muxiva_core::RequestId::new(9),
        session_id: muxiva_types::SessionId::new("session").unwrap(),
        input: text_frame(9, "x"),
        attempt: 1,
    };
    assert!(matches!(
        adapter.send(request),
        muxiva_core::AdapterResponse::Failed(_)
    ));
}
