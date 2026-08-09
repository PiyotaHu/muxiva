use muxiva_types::{Frame, NodeId, SignalFrame};

use crate::PortName;

/// Whether an observed Frame entered or left one Node callback boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameObservationDirection {
    Input,
    Output,
}

/// Whether a graph-local Signal was emitted or delivered to a Node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalObservationDirection {
    Output,
    Input,
}

/// Borrowed description of one Frame crossing a Node port.
///
/// Observers must return quickly. Implementations that write files or encode
/// media should clone the Frame and hand it to a bounded background queue.
#[derive(Clone, Copy)]
pub struct FrameObservation<'a> {
    node_id: &'a NodeId,
    port: &'a PortName,
    direction: FrameObservationDirection,
    frame: &'a Frame,
}

/// Borrowed description of a graph-local Signal at a Node boundary.
#[derive(Clone, Copy)]
pub struct SignalObservation<'a> {
    node_id: &'a NodeId,
    port: Option<&'a PortName>,
    direction: SignalObservationDirection,
    signal: &'a SignalFrame,
}

impl<'a> SignalObservation<'a> {
    pub(crate) const fn new(
        node_id: &'a NodeId,
        port: Option<&'a PortName>,
        direction: SignalObservationDirection,
        signal: &'a SignalFrame,
    ) -> Self {
        Self {
            node_id,
            port,
            direction,
            signal,
        }
    }

    pub const fn node_id(&self) -> &NodeId {
        self.node_id
    }

    pub const fn port(&self) -> Option<&PortName> {
        self.port
    }

    pub const fn direction(&self) -> SignalObservationDirection {
        self.direction
    }

    pub const fn signal(&self) -> &SignalFrame {
        self.signal
    }
}

impl<'a> FrameObservation<'a> {
    pub(crate) const fn new(
        node_id: &'a NodeId,
        port: &'a PortName,
        direction: FrameObservationDirection,
        frame: &'a Frame,
    ) -> Self {
        Self {
            node_id,
            port,
            direction,
            frame,
        }
    }

    pub const fn node_id(&self) -> &NodeId {
        self.node_id
    }

    pub const fn port(&self) -> &PortName {
        self.port
    }

    pub const fn direction(&self) -> FrameObservationDirection {
        self.direction
    }

    pub const fn frame(&self) -> &Frame {
        self.frame
    }
}

/// Optional non-blocking observer for Frames and graph-local Signals at Node boundaries.
///
/// This service is intentionally outside Node business logic. It is suitable
/// for diagnostics such as bounded media dumps and semantic traces and must
/// never be used to alter routing or application behavior.
pub trait RuntimeObserver: Send + Sync {
    fn observe_frame(&self, _observation: FrameObservation<'_>) {}

    fn observe_signal(&self, _observation: SignalObservation<'_>) {}
}
