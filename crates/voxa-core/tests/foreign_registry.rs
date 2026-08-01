use std::sync::{Arc, Mutex};

use voxa_core::{
    ConfigMap, ForeignNodeCallOutput, ForeignNodeFactoryAdapter, ForeignNodeInstance,
    ForeignNodeProvider, NodeFactory, NodeFactoryError, PortName,
};
use voxa_types::{Frame, NodeId, VoxaError};

#[derive(Default)]
struct Calls(Vec<&'static str>);

struct Provider {
    calls: Arc<Mutex<Calls>>,
}

impl ForeignNodeProvider for Provider {
    fn create(
        &self,
        _node_id: &NodeId,
        _config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError> {
        Ok(Box::new(Instance {
            calls: self.calls.clone(),
        }))
    }
}

struct Instance {
    calls: Arc<Mutex<Calls>>,
}

impl ForeignNodeInstance for Instance {
    fn on_prepare(&mut self) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.calls.lock().unwrap().0.push("prepare");
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        input_port: Option<&PortName>,
    ) -> Result<ForeignNodeCallOutput, VoxaError> {
        assert!(input.is_some());
        assert_eq!(input_port.map(PortName::as_str), Some("in"));
        self.calls.lock().unwrap().0.push("process");
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_finish(&mut self) -> Result<ForeignNodeCallOutput, VoxaError> {
        self.calls.lock().unwrap().0.push("finish");
        Ok(ForeignNodeCallOutput::default())
    }
}

#[test]
fn provider_creation_does_not_run_lifecycle_and_adapter_preserves_order() {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let factory = ForeignNodeFactoryAdapter::new(Arc::new(Provider {
        calls: calls.clone(),
    }));
    let mut node = factory
        .create(&NodeId::new("foreign").unwrap(), &ConfigMap::empty())
        .unwrap();
    assert!(calls.lock().unwrap().0.is_empty());

    let node_id = NodeId::new("foreign").unwrap();
    let mut context = voxa_core::NodeContext::new(
        node_id,
        ConfigMap::empty(),
        Some(PortName::new("in").unwrap()),
    );
    node.on_prepare(&mut context).unwrap();
    node.on_process(Some(test_frame()), &mut context).unwrap();
    node.on_finish(&mut context).unwrap();
    assert_eq!(
        calls.lock().unwrap().0.as_slice(),
        ["prepare", "process", "finish"]
    );
}

fn test_frame() -> Frame {
    use voxa_types::{
        ClockDomain, ClockDomainId, ClockKind, Extensions, FrameHeader, FrameId, FramePayload,
        FrameType, Lineage, Metadata, SequenceId, StreamId, TextData, Timestamp, TraceId,
    };
    let header = FrameHeader::new(
        FrameId::new("foreign-frame").unwrap(),
        Timestamp::from_nanos(0),
        ClockDomain::new(
            ClockDomainId::new("foreign-clock").unwrap(),
            ClockKind::Monotonic,
        ),
        SequenceId::new(0),
        StreamId::new("foreign-stream").unwrap(),
        TraceId::new("foreign-trace").unwrap(),
        FrameType::Text,
        Metadata::empty(),
        Extensions::empty(),
        Lineage::empty(),
    )
    .unwrap();
    Frame::new(header, FramePayload::Text(TextData::new("hello"))).unwrap()
}
