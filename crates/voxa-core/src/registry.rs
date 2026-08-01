//! Trusted, versioned node factories and Edge-policy discovery metadata.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Arc,
};

use voxa_types::{FrameType, NodeId};

use crate::{ConfigMap, EdgePolicyName, Node, NodeDescriptor, NodeTypeName};
use crate::{GraphBuildError, GraphBuilder};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NodeLanguage {
    Rust,
    Cpp,
    Python,
    TypeScript,
}

impl NodeLanguage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Cpp => "cpp",
            Self::Python => "python",
            Self::TypeScript => "typescript",
        }
    }
}

/// An exact, stable node-factory contract version.
///
/// Versions are opaque protocol identifiers rather than an ordering promise. A caller must
/// request the exact version written into its graph or selected by a higher-level compiler.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeFactoryVersion(Box<str>);

impl NodeFactoryVersion {
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, NodeFactoryVersionError> {
        let value = value.into();
        if value.is_empty() {
            return Err(NodeFactoryVersionError::Empty);
        }
        if value.len() > 64 {
            return Err(NodeFactoryVersionError::TooLong);
        }
        if value.trim() != value.as_ref() {
            return Err(NodeFactoryVersionError::LeadingOrTrailingWhitespace);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_'))
        {
            return Err(NodeFactoryVersionError::InvalidCharacter);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeFactoryVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeFactoryVersionError {
    Empty,
    TooLong,
    LeadingOrTrailingWhitespace,
    InvalidCharacter,
}

impl fmt::Display for NodeFactoryVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("node factory version must not be empty"),
            Self::TooLong => {
                formatter.write_str("node factory version must be at most 64 bytes")
            }
            Self::LeadingOrTrailingWhitespace => formatter
                .write_str("node factory version must not have leading or trailing whitespace"),
            Self::InvalidCharacter => formatter.write_str(
                "node factory version may contain only ASCII letters, digits, '.', '-', '+', and '_'",
            ),
        }
    }
}

impl Error for NodeFactoryVersionError {}

/// A stable failure returned by trusted factory configuration or creation code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeFactoryError {
    code: Box<str>,
    message: Box<str>,
}

impl NodeFactoryError {
    pub fn new(code: impl Into<Box<str>>, message: impl Into<Box<str>>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for NodeFactoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for NodeFactoryError {}

/// Executable creation boundary stored by [`NodeRegistry`].
///
/// Validation must be deterministic and side-effect free. `create` may allocate resources but
/// must not call a node lifecycle hook; the graph runtime remains the sole lifecycle owner.
pub trait NodeFactory: Send + Sync + 'static {
    fn validate_config(&self, _config: &ConfigMap) -> Result<(), NodeFactoryError> {
        Ok(())
    }

    fn create(
        &self,
        node_id: &NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeFactoryError>;
}

/// One versioned descriptor and its executable factory.
#[derive(Clone)]
pub struct NodeRegistration {
    language: NodeLanguage,
    descriptor: NodeDescriptor,
    version: NodeFactoryVersion,
    factory: Arc<dyn NodeFactory>,
}

impl NodeRegistration {
    pub fn new(
        language: NodeLanguage,
        descriptor: NodeDescriptor,
        version: NodeFactoryVersion,
        factory: Arc<dyn NodeFactory>,
    ) -> Self {
        Self {
            language,
            descriptor,
            version,
            factory,
        }
    }

    pub const fn language(&self) -> NodeLanguage {
        self.language
    }

    pub const fn descriptor(&self) -> &NodeDescriptor {
        &self.descriptor
    }

    pub const fn version(&self) -> &NodeFactoryVersion {
        &self.version
    }

    /// Materializes the registered type-level port shape for a graph-local node ID.
    pub fn descriptor_for(&self, node_id: NodeId) -> NodeDescriptor {
        self.descriptor.for_node_id(node_id)
    }

    fn factory(&self) -> &Arc<dyn NodeFactory> {
        &self.factory
    }
}

impl fmt::Debug for NodeRegistration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeRegistration")
            .field("language", &self.language)
            .field("descriptor", &self.descriptor)
            .field("version", &self.version)
            .field("factory", &"<node factory>")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct EdgePolicyRegistration {
    pub policy: EdgePolicyName,
    pub version: Box<str>,
    pub supported_frame_types: Box<[FrameType]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryError {
    InvalidNodeDescriptor {
        node_type: NodeTypeName,
        source: Box<GraphBuildError>,
    },
    DuplicateNode {
        node_type: NodeTypeName,
        language: NodeLanguage,
        version: NodeFactoryVersion,
    },
    DuplicatePolicy {
        policy: EdgePolicyName,
    },
    UnknownNode {
        node_type: NodeTypeName,
        language: NodeLanguage,
        version: NodeFactoryVersion,
    },
    UnknownPolicy {
        policy: EdgePolicyName,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNodeDescriptor { node_type, source } => {
                write!(
                    formatter,
                    "invalid node factory descriptor `{node_type}`: {source}"
                )
            }
            Self::DuplicateNode {
                node_type,
                language,
                version,
            } => write!(
                formatter,
                "duplicate {:?} node factory `{node_type}` version `{version}`",
                language
            ),
            Self::DuplicatePolicy { policy } => {
                write!(formatter, "duplicate Edge policy `{}`", policy.as_str())
            }
            Self::UnknownNode {
                node_type,
                language,
                version,
            } => write!(
                formatter,
                "unknown {:?} node factory `{node_type}` version `{version}`",
                language
            ),
            Self::UnknownPolicy { policy } => {
                write!(formatter, "unknown Edge policy `{}`", policy.as_str())
            }
        }
    }
}

impl Error for RegistryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidNodeDescriptor { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeCreationStage {
    ValidateConfig,
    Create,
}

/// A bounded, structured node-instantiation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NodeCreateError {
    Registry(RegistryError),
    Factory {
        node_type: NodeTypeName,
        language: NodeLanguage,
        version: NodeFactoryVersion,
        node_id: NodeId,
        stage: NodeCreationStage,
        source: NodeFactoryError,
    },
    FactoryPanicked {
        node_type: NodeTypeName,
        language: NodeLanguage,
        version: NodeFactoryVersion,
        node_id: NodeId,
        stage: NodeCreationStage,
    },
}

impl fmt::Display for NodeCreateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(error) => error.fmt(formatter),
            Self::Factory {
                node_type,
                language,
                version,
                node_id,
                stage,
                source,
            } => write!(
                formatter,
                "{:?} failed for {:?} node factory `{node_type}` version `{version}` at node `{node_id}`: {source}",
                stage, language
            ),
            Self::FactoryPanicked {
                node_type,
                language,
                version,
                node_id,
                stage,
            } => write!(
                formatter,
                "{:?} panicked for {:?} node factory `{node_type}` version `{version}` at node `{node_id}`",
                stage, language
            ),
        }
    }
}

impl Error for NodeCreateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Registry(error) => Some(error),
            Self::Factory { source, .. } => Some(source),
            Self::FactoryPanicked { .. } => None,
        }
    }
}

impl From<RegistryError> for NodeCreateError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

type NodeRegistryKey = (NodeTypeName, NodeLanguage, NodeFactoryVersion);

#[derive(Default)]
pub struct NodeRegistry {
    entries: BTreeMap<NodeRegistryKey, NodeRegistration>,
}

impl NodeRegistry {
    pub fn register(&mut self, registration: NodeRegistration) -> Result<(), RegistryError> {
        let node_type = registration.descriptor().node_type().clone();
        GraphBuilder::new()
            .add_node(registration.descriptor().clone())
            .map_err(|source| RegistryError::InvalidNodeDescriptor {
                node_type: node_type.clone(),
                source: Box::new(source),
            })?;
        let key = (
            node_type,
            registration.language(),
            registration.version().clone(),
        );
        if self.entries.contains_key(&key) {
            return Err(RegistryError::DuplicateNode {
                node_type: key.0,
                language: key.1,
                version: key.2,
            });
        }
        self.entries.insert(key, registration);
        Ok(())
    }

    pub fn resolve(
        &self,
        node_type: &NodeTypeName,
        language: NodeLanguage,
        version: &NodeFactoryVersion,
    ) -> Result<&NodeRegistration, RegistryError> {
        self.entries
            .get(&(node_type.clone(), language, version.clone()))
            .ok_or_else(|| RegistryError::UnknownNode {
                node_type: node_type.clone(),
                language,
                version: version.clone(),
            })
    }

    /// Resolves and materializes type-level metadata for one graph-local node.
    pub fn descriptor_for(
        &self,
        node_type: &NodeTypeName,
        language: NodeLanguage,
        version: &NodeFactoryVersion,
        node_id: NodeId,
    ) -> Result<NodeDescriptor, RegistryError> {
        Ok(self
            .resolve(node_type, language, version)?
            .descriptor_for(node_id))
    }

    /// Runs deterministic factory validation without allocating a node instance.
    pub fn validate_config(
        &self,
        node_type: &NodeTypeName,
        language: NodeLanguage,
        version: &NodeFactoryVersion,
        node_id: NodeId,
        config: &ConfigMap,
    ) -> Result<(), NodeCreateError> {
        let registration = self.resolve(node_type, language, version)?;
        let factory = registration.factory();
        match catch_unwind(AssertUnwindSafe(|| factory.validate_config(config))) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(source)) => Err(NodeCreateError::Factory {
                node_type: node_type.clone(),
                language,
                version: version.clone(),
                node_id,
                stage: NodeCreationStage::ValidateConfig,
                source,
            }),
            Err(_) => Err(NodeCreateError::FactoryPanicked {
                node_type: node_type.clone(),
                language,
                version: version.clone(),
                node_id,
                stage: NodeCreationStage::ValidateConfig,
            }),
        }
    }

    /// Validates configuration and creates a fresh runtime instance without running lifecycle.
    pub fn create(
        &self,
        node_type: &NodeTypeName,
        language: NodeLanguage,
        version: &NodeFactoryVersion,
        node_id: NodeId,
        config: &ConfigMap,
    ) -> Result<Box<dyn Node>, NodeCreateError> {
        let registration = self.resolve(node_type, language, version)?;
        let factory = registration.factory();
        let identity = || (node_type.clone(), version.clone(), node_id.clone());
        self.validate_config(node_type, language, version, node_id.clone(), config)?;

        match catch_unwind(AssertUnwindSafe(|| factory.create(&node_id, config))) {
            Ok(Ok(node)) => Ok(node),
            Ok(Err(source)) => {
                let (node_type, version, node_id) = identity();
                Err(NodeCreateError::Factory {
                    node_type,
                    language,
                    version,
                    node_id,
                    stage: NodeCreationStage::Create,
                    source,
                })
            }
            Err(_) => {
                let (node_type, version, node_id) = identity();
                Err(NodeCreateError::FactoryPanicked {
                    node_type,
                    language,
                    version,
                    node_id,
                    stage: NodeCreationStage::Create,
                })
            }
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &NodeRegistration> {
        self.entries.values()
    }
}

#[derive(Default)]
pub struct EdgePolicyRegistry {
    entries: BTreeMap<EdgePolicyName, EdgePolicyRegistration>,
}

impl EdgePolicyRegistry {
    pub fn register(&mut self, registration: EdgePolicyRegistration) -> Result<(), RegistryError> {
        if self.entries.contains_key(&registration.policy) {
            return Err(RegistryError::DuplicatePolicy {
                policy: registration.policy,
            });
        }
        self.entries
            .insert(registration.policy.clone(), registration);
        Ok(())
    }

    pub fn resolve(
        &self,
        policy: &EdgePolicyName,
    ) -> Result<&EdgePolicyRegistration, RegistryError> {
        self.entries
            .get(policy)
            .ok_or_else(|| RegistryError::UnknownPolicy {
                policy: policy.clone(),
            })
    }

    pub fn entries(&self) -> impl Iterator<Item = &EdgePolicyRegistration> {
        self.entries.values()
    }
}
