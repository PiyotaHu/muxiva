use muxiva_types::{Frame, NodeId, Result, SignalFrame};

use crate::{EdgeDescriptor, EdgeMetricsSnapshot, GraphDefinition};

/// Read-only information available to one Edge policy callback.
///
/// The context deliberately exposes no runner, destination node, work list,
/// or mutable metrics access.
pub struct EdgeContext<'a> {
    graph: EdgeGraphContext<'a>,
    descriptor: &'a EdgeDescriptor,
    metrics: &'a EdgeMetricsSnapshot,
}

impl<'a> EdgeContext<'a> {
    pub(crate) fn new(
        graph: &'a GraphDefinition,
        descriptor: &'a EdgeDescriptor,
        metrics: &'a EdgeMetricsSnapshot,
    ) -> Self {
        Self {
            graph: EdgeGraphContext {
                node_count: graph.nodes().len(),
                edge_count: graph.edges().len(),
                topological_order: graph.topological_order(),
            },
            descriptor,
            metrics,
        }
    }

    /// Returns immutable graph data for stable identity and diagnostics.
    pub const fn graph(&self) -> &EdgeGraphContext<'a> {
        &self.graph
    }

    /// Returns the current immutable Edge descriptor.
    pub const fn descriptor(&self) -> &EdgeDescriptor {
        self.descriptor
    }

    /// Returns a coherent read-only snapshot of this Edge's metrics.
    pub const fn metrics(&self) -> &EdgeMetricsSnapshot {
        self.metrics
    }
}

/// Bounded read-only graph information available to an Edge policy.
///
/// It intentionally cannot resolve node or Edge descriptors; the current Edge
/// descriptor is available separately through [`EdgeContext::descriptor`].
pub struct EdgeGraphContext<'a> {
    node_count: usize,
    edge_count: usize,
    topological_order: &'a [NodeId],
}

impl EdgeGraphContext<'_> {
    /// Returns the number of declared nodes.
    pub const fn node_count(&self) -> usize {
        self.node_count
    }

    /// Returns the number of declared Edges.
    pub const fn edge_count(&self) -> usize {
        self.edge_count
    }

    /// Returns the stable node topology for bounded graph diagnostics.
    pub const fn topological_order(&self) -> &[NodeId] {
        self.topological_order
    }
}

/// The result of the validation step of an Edge policy pipeline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationDecision {
    /// Continue to the transform step.
    Accept,
    /// Apply the Edge descriptor's configured validation-failure action.
    Reject(Box<str>),
}

/// The complete set of Stage 4 Edge transform dispositions.
#[derive(Clone, Eq, PartialEq)]
pub enum EdgeAction {
    /// Deliver the unchanged input frame.
    Forward(Frame),
    /// Deliver a distinct replacement after automatic Edge lineage is added.
    Replace(Frame),
    /// Observe and discard the frame.
    Drop(Box<str>),
    /// Stop the graph with this non-sensitive reason.
    Abort(Box<str>),
    /// Observe a Signal and invoke the policy hook without node-level routing.
    EmitSignal(Frame),
}

/// Runtime Edge behavior attached separately from a pure graph definition.
///
/// A named validation selection calls [`Self::validate`] before a named
/// transform selection calls [`Self::transform`]. Hooks execute synchronously
/// and are protected by the runner's panic boundary.
pub trait EdgePolicy: Send {
    /// Validates an immutable candidate frame.
    fn validate(
        &mut self,
        _frame: &Frame,
        _context: &EdgeContext<'_>,
    ) -> Result<ValidationDecision> {
        Ok(ValidationDecision::Accept)
    }

    /// Produces the Edge disposition for an accepted immutable frame.
    fn transform(&mut self, frame: &Frame, _context: &EdgeContext<'_>) -> Result<EdgeAction> {
        Ok(EdgeAction::Forward(frame.clone()))
    }

    /// Observes a Stage 4 signal action on this same Edge.
    ///
    /// Adjacent node delivery is intentionally deferred to Stage 6.
    fn on_signal(&mut self, _signal: &SignalFrame, _context: &EdgeContext<'_>) -> Result<()> {
        Ok(())
    }

    /// Observes exactly one validation or explicit policy drop.
    fn on_drop(&mut self, _reason: &str, _context: &EdgeContext<'_>) -> Result<()> {
        Ok(())
    }
}
