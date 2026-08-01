#![forbid(unsafe_code)]
//! Runtime-facing services built on Voxa's public foundation types.

pub mod admission;
pub mod audio_merge;
pub mod cancellation;
pub mod concurrent;
pub mod edge;
pub mod edge_policy;
pub mod event_bus;
pub mod flow;
pub mod foreign;
pub mod foreign_registry;
pub mod graph;
pub mod logging;
pub mod managed_stream;
pub mod node;
pub mod queue;
pub mod realtime;
pub mod registered_runtime;
pub mod registry;
pub mod resource;
pub mod runner;
mod signal;
pub mod transport;

pub use admission::{AdmissionError, AdmissionLease, AdmissionSlots, AdmissionSnapshot};
pub use audio_merge::{merge_audio_prefix, FrameIdSource, MergedAudioFrame};
pub use cancellation::{Cancellation, StopToken};
pub use concurrent::{
    ConcurrentRunSummary, ConcurrentRuntime, ConcurrentRuntimeState, GraphRuntime, RuntimeOptions,
    RuntimeStartError, RuntimeThreadRole, RuntimeWaitError, ShutdownDiagnostics,
};
pub use edge::{
    EdgeDescriptor, EdgeMetrics, EdgeMetricsSnapshot, EdgePolicyName, EnabledCondition,
    QueueOverflowPolicy, QueuePolicy, TransformPolicy, ValidationFailureAction, ValidationPolicy,
    VisibilityDescriptor, VisibilityLabel, VisibilityLevel,
};
pub use edge_policy::{EdgeAction, EdgeContext, EdgeGraphContext, EdgePolicy, ValidationDecision};
pub use event_bus::{
    EventBus, EventBusError, EventBusStopReport, PublishReport, SubscriberSnapshot, Subscription,
};
pub use flow::{
    overflow_may_drop, AdaptiveFlowController, FlowAction, FlowClock, FlowDropReason, FlowError,
    FlowSignalObservation, FlowSnapshot, FlowState, FlowUpdate, FlowWork, FrameMeasurement,
    InputPortKey, OverflowDecision, TrustedVadDecision,
};
pub use foreign::{
    ForeignCommand, ForeignCommandKind, ForeignCompletion, ForeignCompletionEmission,
    ForeignCompletionKind, ForeignCompletionOutcome, ForeignDriverConfig, ForeignDriverError,
    ForeignDriverSnapshot, ForeignFullPolicy, ForeignNodeDriver, ForeignOrdering,
    ForeignShutdownDiagnostics, ForeignSubmitOutcome,
};
pub use foreign_registry::{
    ForeignNodeCallOutput, ForeignNodeEmission, ForeignNodeFactoryAdapter, ForeignNodeInstance,
    ForeignNodeProvider,
};
pub use graph::{
    EdgeEndpoint, EndpointRole, GraphBuildError, GraphBuilder, GraphDefinition, NodeDefinition,
};
pub use managed_stream::{
    AdapterRequest, AdapterResponse, AsyncRequest, ManagedAsyncStream, ManagedStreamAdapter,
    ManagedStreamBuildError, ManagedStreamMetricsSnapshot, ManagedStreamOptions,
    ManagedStreamState, ManagedStreamStopReport, RequestId, ServiceError, StreamCompletion,
    StreamResult, SubmitOutcome,
};
pub use node::{
    AbortCategory, AbortReason, AbortRootContext, AbortStage, ConfigKey, ConfigMap, ConfigSchema,
    DescriptorNameError, DuplicateConfigKey, LifecycleCapabilities, Node, NodeContext,
    NodeDescriptor, NodeEmission, NodeEmissionError, NodeKind, NodeTypeName, PortDescriptor,
    PortDirection, PortName, SignalEmissionError,
};
pub use queue::{
    DrainMode, EdgeQueue, EnqueueOutcome, QueueClosed, QueueDropReason, QueuePushError,
};
pub use realtime::{
    AudioDurationRange, AudioOverflowPolicy, DeliveryGuarantee, DeliveryOrdering, RealtimeContract,
    RealtimeInputProfile, RealtimeProfileError, RuntimeInputTuning,
};
pub use registered_runtime::{
    materialize_registered_nodes, start_registered_runtime, GraphMaterializationError,
    RegisteredRuntimeStartError,
};
pub use registry::{
    EdgePolicyRegistration, EdgePolicyRegistry, NodeCreateError, NodeCreationStage, NodeFactory,
    NodeFactoryError, NodeFactorySelection, NodeFactoryVersion, NodeFactoryVersionError,
    NodeLanguage, NodeRegistration, NodeRegistry, RegistryError,
};
pub use resource::{ResourceKey, ResourceStore, ResourceStoreError};
pub use runner::{
    AbortHookDiagnostic, EdgePolicies, GraphRunSummary, GraphRunner, GraphRunnerBuildError,
    GraphRunnerState, NodeInstances, ObservedEdgeSignal,
};
pub use signal::{SignalQueuePushError, SignalQueueSnapshot};
pub use transport::{
    ConnectionState, ControlApplyOutcome, TransportControl, TransportControlError,
    TransportSnapshot,
};
