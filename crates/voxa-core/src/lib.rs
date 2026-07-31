#![forbid(unsafe_code)]
//! Runtime-facing services built on Voxa's public foundation types.

pub mod edge;
pub mod edge_policy;
pub mod graph;
pub mod logging;
pub mod node;
pub mod runner;

pub use edge::{
    EdgeDescriptor, EdgeMetrics, EdgeMetricsSnapshot, EdgePolicyName, EnabledCondition,
    QueueOverflowPolicy, QueuePolicy, TransformPolicy, ValidationFailureAction, ValidationPolicy,
    VisibilityDescriptor, VisibilityLabel, VisibilityLevel,
};
pub use edge_policy::{EdgeAction, EdgeContext, EdgeGraphContext, EdgePolicy, ValidationDecision};
pub use graph::{
    EdgeEndpoint, EndpointRole, GraphBuildError, GraphBuilder, GraphDefinition, NodeDefinition,
};
pub use node::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigKey, ConfigMap, ConfigSchema,
    DescriptorNameError, DuplicateConfigKey, LifecycleCapabilities, Node, NodeContext,
    NodeDescriptor, NodeEmission, NodeKind, NodeTypeName, PortDescriptor, PortDirection, PortName,
};
pub use runner::{
    AbortHookDiagnostic, EdgePolicies, GraphRunSummary, GraphRunner, GraphRunnerBuildError,
    GraphRunnerState, NodeInstances, ObservedEdgeSignal,
};
