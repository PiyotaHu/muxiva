use std::sync::{Arc, Mutex};
use voxa_core::{AbortReason, Node, NodeContext, PortName};
use voxa_types::{Frame, Result, SignalFrame};
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleCall {
    Prepare,
    Process(u64),
    Signal,
    Finish,
    Abort,
}
pub struct TestNode {
    log: Arc<Mutex<Vec<LifecycleCall>>>,
    output: Option<PortName>,
    fail_process: bool,
}
impl TestNode {
    pub fn new(log: Arc<Mutex<Vec<LifecycleCall>>>) -> Self {
        Self {
            log,
            output: None,
            fail_process: false,
        }
    }
    pub fn emitting(mut self, port: PortName) -> Self {
        self.output = Some(port);
        self
    }
    pub fn failing_process(mut self) -> Self {
        self.fail_process = true;
        self
    }
}
impl Node for TestNode {
    fn on_prepare(&mut self, _: &mut NodeContext) -> Result<()> {
        self.log.lock().unwrap().push(LifecycleCall::Prepare);
        Ok(())
    }
    fn on_process(&mut self, input: Option<Frame>, ctx: &mut NodeContext) -> Result<()> {
        let seq = input.as_ref().map_or(0, |f| f.header().sequence_id().get());
        self.log.lock().unwrap().push(LifecycleCall::Process(seq));
        if self.fail_process {
            return Err(voxa_types::VoxaError::new(
                voxa_types::ErrorCategory::Internal,
                "VOXA-TEST-NODE",
                "scripted process failure",
            ));
        }
        if let (Some(frame), Some(port)) = (input, self.output.clone()) {
            ctx.emit(port, frame)?
        }
        Ok(())
    }
    fn on_signal(&mut self, _: SignalFrame, _: &mut NodeContext) -> Result<()> {
        self.log.lock().unwrap().push(LifecycleCall::Signal);
        Ok(())
    }
    fn on_finish(&mut self, _: &mut NodeContext) -> Result<()> {
        self.log.lock().unwrap().push(LifecycleCall::Finish);
        Ok(())
    }
    fn on_abort(&mut self, _: &AbortReason, _: &mut NodeContext) {
        self.log.lock().unwrap().push(LifecycleCall::Abort)
    }
}
pub struct TestSource {
    frames: Vec<Frame>,
    output: PortName,
}
impl TestSource {
    pub fn new(frames: Vec<Frame>, output: PortName) -> Self {
        Self { frames, output }
    }
}
impl Node for TestSource {
    fn on_process(&mut self, _: Option<Frame>, ctx: &mut NodeContext) -> Result<()> {
        for frame in self.frames.drain(..) {
            ctx.emit(self.output.clone(), frame)?
        }
        Ok(())
    }
}
pub struct TestSink {
    frames: Arc<Mutex<Vec<Frame>>>,
}
impl TestSink {
    pub fn new(frames: Arc<Mutex<Vec<Frame>>>) -> Self {
        Self { frames }
    }
}
impl Node for TestSink {
    fn on_process(&mut self, input: Option<Frame>, _: &mut NodeContext) -> Result<()> {
        if let Some(frame) = input {
            self.frames.lock().unwrap().push(frame)
        }
        Ok(())
    }
}
