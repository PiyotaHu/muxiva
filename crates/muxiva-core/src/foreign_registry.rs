//! Language-hosted Node factories adapted to the common Registry contract.

use std::{sync::Arc, time::Duration};

use muxiva_types::{EventFrame, Frame, MuxivaError, NodeId, SignalFrame};

use crate::{AbortReason, ConfigMap, Node, NodeContext, NodeFactory, NodeFactoryError, PortName};

/// One owned frame emission returned by a language-hosted lifecycle call.
pub struct ForeignNodeEmission {
    output_port: PortName,
    frame: Frame,
}

impl ForeignNodeEmission {
    pub fn new(output_port: PortName, frame: Frame) -> Self {
        Self { output_port, frame }
    }

    pub fn output_port(&self) -> &PortName {
        &self.output_port
    }

    pub const fn frame(&self) -> &Frame {
        &self.frame
    }
}

/// Owned output from one language-hosted Node call.
#[derive(Default)]
pub struct ForeignNodeCallOutput {
    emissions: Vec<ForeignNodeEmission>,
    signals: Vec<SignalFrame>,
    events: Vec<EventFrame>,
    next_source_tick: Option<Duration>,
}

impl ForeignNodeCallOutput {
    pub fn new(
        emissions: impl Into<Vec<ForeignNodeEmission>>,
        signals: impl Into<Vec<SignalFrame>>,
    ) -> Self {
        Self {
            emissions: emissions.into(),
            signals: signals.into(),
            events: Vec::new(),
            next_source_tick: None,
        }
    }

    pub fn with_events(mut self, events: impl Into<Vec<EventFrame>>) -> Self {
        self.events = events.into();
        self
    }

    pub fn with_next_source_tick(mut self, delay: Duration) -> Self {
        self.next_source_tick = Some(delay);
        self
    }

    pub fn with_signals(mut self, signals: impl Into<Vec<SignalFrame>>) -> Self {
        self.signals = signals.into();
        self
    }

    pub fn with_additional_emissions(
        mut self,
        emissions: impl IntoIterator<Item = ForeignNodeEmission>,
    ) -> Self {
        self.emissions.extend(emissions);
        self
    }

    pub fn from_frame(output_port: PortName, frame: Frame) -> Self {
        Self::new([ForeignNodeEmission::new(output_port, frame)], [])
    }

    pub fn emissions(&self) -> &[ForeignNodeEmission] {
        &self.emissions
    }

    pub fn signals(&self) -> &[SignalFrame] {
        &self.signals
    }

    pub fn events(&self) -> &[EventFrame] {
        &self.events
    }
}

/// One fresh language-owned Node instance.
///
/// Implementations may dispatch into a dedicated interpreter thread, Worker,
/// or C ABI object. Every call is made from the owning graph worker and must
/// return before its configured language deadline.
pub trait ForeignNodeInstance: Send + 'static {
    fn on_prepare(&mut self) -> Result<ForeignNodeCallOutput, MuxivaError> {
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        input_port: Option<&PortName>,
    ) -> Result<ForeignNodeCallOutput, MuxivaError>;

    fn on_signal(&mut self, _signal: SignalFrame) -> Result<ForeignNodeCallOutput, MuxivaError> {
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_finish(&mut self) -> Result<ForeignNodeCallOutput, MuxivaError> {
        Ok(ForeignNodeCallOutput::default())
    }

    fn on_abort(&mut self, _reason: &AbortReason) {}
}

/// Trusted host-side constructor for fresh foreign Node instances.
pub trait ForeignNodeConstructor: Send + Sync + 'static {
    fn validate_config(&self, _config: &ConfigMap) -> Result<(), NodeFactoryError> {
        Ok(())
    }

    fn create(
        &self,
        node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn ForeignNodeInstance>, NodeFactoryError>;
}

/// Adapts a language-hosted constructor into the executable [`NodeFactory`] contract.
pub struct ForeignNodeFactoryAdapter {
    constructor: Arc<dyn ForeignNodeConstructor>,
}

impl ForeignNodeFactoryAdapter {
    pub fn new(constructor: Arc<dyn ForeignNodeConstructor>) -> Self {
        Self { constructor }
    }
}

impl NodeFactory for ForeignNodeFactoryAdapter {
    fn validate_config(&self, config: &ConfigMap) -> Result<(), NodeFactoryError> {
        self.constructor.validate_config(config)
    }

    fn create(
        &self,
        node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError> {
        Ok(Box::new(ForeignNodeAdapter {
            instance: self.constructor.create(node_id, config)?,
        }))
    }
}

struct ForeignNodeAdapter {
    instance: Box<dyn ForeignNodeInstance>,
}

impl ForeignNodeAdapter {
    fn apply(output: ForeignNodeCallOutput, context: &mut NodeContext) -> Result<(), MuxivaError> {
        for emission in output.emissions {
            context.emit(emission.output_port, emission.frame)?;
        }
        for signal in output.signals {
            context.emit_signal(signal)?;
        }
        for event in output.events {
            context.publish_notification(event)?;
        }
        if let Some(delay) = output.next_source_tick {
            context.schedule_next_tick(delay);
        }
        Ok(())
    }
}

impl Node for ForeignNodeAdapter {
    fn on_prepare(&mut self, context: &mut NodeContext) -> Result<(), MuxivaError> {
        Self::apply(self.instance.on_prepare()?, context)
    }

    fn on_process(
        &mut self,
        input: Option<Frame>,
        context: &mut NodeContext,
    ) -> Result<(), MuxivaError> {
        let output = self.instance.on_process(input, context.input_port())?;
        Self::apply(output, context)
    }

    fn on_signal(
        &mut self,
        signal: SignalFrame,
        context: &mut NodeContext,
    ) -> Result<(), MuxivaError> {
        Self::apply(self.instance.on_signal(signal)?, context)
    }

    fn on_finish(&mut self, context: &mut NodeContext) -> Result<(), MuxivaError> {
        Self::apply(self.instance.on_finish()?, context)
    }

    fn on_abort(&mut self, reason: &AbortReason, _context: &mut NodeContext) {
        self.instance.on_abort(reason);
    }
}
