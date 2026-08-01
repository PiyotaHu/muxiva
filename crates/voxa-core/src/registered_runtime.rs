//! Registry-selected graph materialization and concurrent Runtime startup.

use std::{collections::BTreeMap, error::Error, fmt};

use voxa_types::NodeId;

use crate::{
    ConcurrentRuntime, EdgePolicies, GraphDefinition, GraphRunnerBuildError, GraphRuntime,
    NodeCreateError, NodeInstances, NodeRegistry, NodeTypeName, RuntimeOptions, RuntimeStartError,
};

/// A compiled graph could not be converted into an exact runtime Node map.
#[derive(Debug)]
pub enum GraphMaterializationError {
    MissingFactorySelection {
        node_id: NodeId,
        node_type: NodeTypeName,
    },
    NodeCreation {
        node_id: NodeId,
        source: NodeCreateError,
    },
}

impl fmt::Display for GraphMaterializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFactorySelection { node_id, node_type } => write!(
                formatter,
                "node `{node_id}` of type `{node_type}` has no exact Factory selection"
            ),
            Self::NodeCreation { node_id, source } => {
                write!(
                    formatter,
                    "failed to materialize node `{node_id}`: {source}"
                )
            }
        }
    }
}

impl Error for GraphMaterializationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MissingFactorySelection { .. } => None,
            Self::NodeCreation { source, .. } => Some(source),
        }
    }
}

/// A failure before the concurrent graph reached its Running state.
#[derive(Debug)]
pub enum RegisteredRuntimeStartError {
    Materialization(GraphMaterializationError),
    Attachments(GraphRunnerBuildError),
    Threads(RuntimeStartError),
}

impl fmt::Display for RegisteredRuntimeStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Materialization(error) => error.fmt(formatter),
            Self::Attachments(error) => write!(formatter, "invalid runtime attachments: {error}"),
            Self::Threads(error) => error.fmt(formatter),
        }
    }
}

impl Error for RegisteredRuntimeStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Materialization(error) => Some(error),
            Self::Attachments(error) => Some(error),
            Self::Threads(error) => Some(error),
        }
    }
}

/// Creates every selected Node without invoking lifecycle callbacks.
pub fn materialize_registered_nodes(
    graph: &GraphDefinition,
    registry: &NodeRegistry,
) -> Result<NodeInstances, GraphMaterializationError> {
    let mut instances = BTreeMap::new();
    for definition in graph.nodes() {
        let descriptor = definition.descriptor();
        let node_id = descriptor.node_id().clone();
        let selection = definition.factory().ok_or_else(|| {
            GraphMaterializationError::MissingFactorySelection {
                node_id: node_id.clone(),
                node_type: descriptor.node_type().clone(),
            }
        })?;
        let node = registry
            .create(
                descriptor.node_type(),
                selection.language(),
                selection.version(),
                node_id.clone(),
                definition.config(),
            )
            .map_err(|source| GraphMaterializationError::NodeCreation {
                node_id: node_id.clone(),
                source,
            })?;
        instances.insert(node_id, node);
    }
    Ok(instances)
}

/// Materializes and starts one compiled graph through the general concurrent Runtime.
pub fn start_registered_runtime(
    graph: GraphDefinition,
    registry: &NodeRegistry,
    policies: EdgePolicies,
    options: RuntimeOptions,
) -> Result<GraphRuntime, RegisteredRuntimeStartError> {
    let nodes = materialize_registered_nodes(&graph, registry)
        .map_err(RegisteredRuntimeStartError::Materialization)?;
    ConcurrentRuntime::new(graph, nodes, policies, options)
        .map_err(RegisteredRuntimeStartError::Attachments)?
        .start()
        .map_err(RegisteredRuntimeStartError::Threads)
}
