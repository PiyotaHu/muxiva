#![forbid(unsafe_code)]
//! Runtime-facing services built on Voxa's public foundation types.

pub mod admission;
pub mod audio_merge;
pub mod cancellation;
pub mod concurrent;
pub mod edge;
pub mod edge_policy;
pub mod flow;
pub mod graph;
pub mod logging;
pub mod node;
pub mod queue;
pub mod realtime;
pub mod runner;

pub use admission::{AdmissionError, AdmissionLease, AdmissionSlots, AdmissionSnapshot};
pub use audio_merge::{merge_audio_prefix, FrameIdSource, MergedAudioFrame};
pub use cancellation::{Cancellation, StopToken};
pub use concurrent::{
    ConcurrentRunSummary, ConcurrentRuntime, ConcurrentRuntimeState, GraphRuntime, RuntimeOptions,
    RuntimeWaitError, ShutdownDiagnostics,
};
pub use edge::{
    EdgeDescriptor, EdgeMetrics, EdgeMetricsSnapshot, EdgePolicyName, EnabledCondition,
    QueueOverflowPolicy, QueuePolicy, TransformPolicy, ValidationFailureAction, ValidationPolicy,
    VisibilityDescriptor, VisibilityLabel, VisibilityLevel,
};
pub use edge_policy::{EdgeAction, EdgeContext, EdgeGraphContext, EdgePolicy, ValidationDecision};
pub use flow::{
    overflow_may_drop, AdaptiveFlowController, FlowAction, FlowClock, FlowDropReason, FlowError,
    FlowSignalObservation, FlowSnapshot, FlowState, FlowUpdate, FlowWork, FrameMeasurement,
    InputPortKey, OverflowDecision, TrustedVadDecision,
};
pub use graph::{
    EdgeEndpoint, EndpointRole, GraphBuildError, GraphBuilder, GraphDefinition, NodeDefinition,
};
pub use node::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigKey, ConfigMap, ConfigSchema,
    DescriptorNameError, DuplicateConfigKey, LifecycleCapabilities, Node, NodeContext,
    NodeDescriptor, NodeEmission, NodeKind, NodeTypeName, PortDescriptor, PortDirection, PortName,
};
pub use queue::{
    DrainMode, EdgeQueue, EnqueueOutcome, QueueClosed, QueueDropReason, QueuePushError,
};
pub use realtime::{
    AudioDurationRange, AudioOverflowPolicy, DeliveryGuarantee, DeliveryOrdering, RealtimeContract,
    RealtimeInputProfile, RealtimeProfileError, RuntimeInputTuning,
};
pub use runner::{
    AbortHookDiagnostic, EdgePolicies, GraphRunSummary, GraphRunner, GraphRunnerBuildError,
    GraphRunnerState, NodeInstances, ObservedEdgeSignal,
};
