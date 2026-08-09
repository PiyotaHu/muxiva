#![forbid(unsafe_code)]
//! Runtime-facing services built on Muxiva's public foundation types.

pub mod admission;
pub mod audio_merge;
pub mod cancellation;
pub mod concurrent;
pub mod edge;
pub mod edge_policy;
pub mod flow;
pub mod foreign;
pub mod foreign_registry;
pub mod graph;
pub mod logging;
pub mod managed_stream;
pub mod node;
pub mod notification_bus;
pub mod queue;
pub mod realtime;
pub mod registered_runtime;
pub mod registry;
pub mod resource;
pub mod runner;
pub mod runtime_observer;
mod signal;

pub use admission::{AdmissionError, AdmissionLease, AdmissionSlots, AdmissionSnapshot};
pub use audio_merge::{merge_audio_prefix, FrameIdSource, MergedAudioFrame};
pub use cancellation::{Cancellation, StopToken};
pub use concurrent::{
    ConcurrentRunSummary, ConcurrentRuntime, ConcurrentRuntimeState, GraphRuntime,
    NodeCustomMetricSnapshot, NodeMetricsSnapshot, RuntimeOptions, RuntimeStartError,
    RuntimeThreadRole, RuntimeWaitError, ShutdownDiagnostics,
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
pub use foreign::{
    ForeignCommand, ForeignCommandKind, ForeignCompletion, ForeignCompletionEmission,
    ForeignCompletionKind, ForeignCompletionOutcome, ForeignDriverConfig, ForeignDriverError,
    ForeignDriverSnapshot, ForeignFullPolicy, ForeignNodeDriver, ForeignOrdering,
    ForeignShutdownDiagnostics, ForeignSubmitOutcome,
};
pub use foreign_registry::{
    ForeignNodeCallOutput, ForeignNodeConstructor, ForeignNodeEmission, ForeignNodeFactoryAdapter,
    ForeignNodeInstance,
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
    NodeDescriptor, NodeEmission, NodeEmissionError, NodeKind, NodeMetricError, NodeMetricKind,
    NodeMetricObservation, NodeTypeName, PortDescriptor, PortDirection, PortName,
    SignalEmissionError,
};
pub use notification_bus::{
    NotificationBus, NotificationBusError, NotificationBusStopReport, PublishReport,
    SubscriberSnapshot, Subscription,
};
pub use queue::{
    DrainMode, EdgeQueue, EnqueueOutcome, QueueClosed, QueueDropReason, QueuePushError,
};
pub use realtime::{
    AudioDurationRange, AudioOverflowPolicy, DeliveryGuarantee, DeliveryOrdering, RealtimeContract,
    RealtimeInputProfile, RealtimeProfileError, RuntimeInputTuning,
};
pub use registered_runtime::{
    materialize_registered_nodes, start_registered_runtime, start_registered_runtime_with_context,
    start_registered_runtime_with_context_and_observer, start_registered_runtime_with_resources,
    GraphMaterializationError, RegisteredRuntimeStartError,
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
pub use runtime_observer::{
    FrameObservation, FrameObservationDirection, RuntimeObserver, SignalObservation,
    SignalObservationDirection,
};
pub use signal::{SignalQueuePushError, SignalQueueSnapshot};
