use muxiva_core::{EdgeAction, EdgeContext, EdgePolicy, ValidationDecision};
use muxiva_types::{Frame, Result};
use std::sync::{Arc, Mutex};
#[derive(Clone, Debug)]
pub enum TestEdgeDisposition {
    Forward,
    Drop(Box<str>),
    Abort(Box<str>),
    Reject(Box<str>),
}
pub struct TestEdgePolicy {
    script: Vec<TestEdgeDisposition>,
    calls: Arc<Mutex<usize>>,
}
impl TestEdgePolicy {
    pub fn new(script: Vec<TestEdgeDisposition>, calls: Arc<Mutex<usize>>) -> Self {
        Self { script, calls }
    }
    fn next(&mut self) -> TestEdgeDisposition {
        if self.script.is_empty() {
            TestEdgeDisposition::Forward
        } else {
            self.script.remove(0)
        }
    }
}
impl EdgePolicy for TestEdgePolicy {
    fn validate(&mut self, _: &Frame, _: &EdgeContext<'_>) -> Result<ValidationDecision> {
        *self.calls.lock().unwrap() += 1;
        match self.next() {
            TestEdgeDisposition::Reject(r) => Ok(ValidationDecision::Reject(r)),
            other => {
                self.script.insert(0, other);
                Ok(ValidationDecision::Accept)
            }
        }
    }
    fn transform(&mut self, frame: &Frame, _: &EdgeContext<'_>) -> Result<EdgeAction> {
        Ok(match self.next() {
            TestEdgeDisposition::Forward => EdgeAction::Forward(frame.clone()),
            TestEdgeDisposition::Drop(r) => EdgeAction::Drop(r),
            TestEdgeDisposition::Abort(r) => EdgeAction::Abort(r),
            TestEdgeDisposition::Reject(r) => EdgeAction::Drop(r),
        })
    }
}
