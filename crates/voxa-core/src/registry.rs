//! Trusted, compiled-in node and Edge-policy discovery metadata.
use std::{collections::BTreeMap, error::Error, fmt};

use voxa_types::FrameType;

use crate::{EdgePolicyName, NodeDescriptor, NodeTypeName};

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

#[derive(Clone, Debug)]
pub struct NodeRegistration {
    pub language: NodeLanguage,
    pub descriptor: NodeDescriptor,
    pub version: Box<str>,
}
#[derive(Clone, Debug)]
pub struct EdgePolicyRegistration {
    pub policy: EdgePolicyName,
    pub version: Box<str>,
    pub supported_frame_types: Box<[FrameType]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistryError {
    DuplicateNode {
        node_type: NodeTypeName,
        language: NodeLanguage,
    },
    DuplicatePolicy {
        policy: EdgePolicyName,
    },
    UnknownNode {
        node_type: NodeTypeName,
        language: NodeLanguage,
    },
    UnknownPolicy {
        policy: EdgePolicyName,
    },
}
impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for RegistryError {}

#[derive(Default)]
pub struct NodeRegistry {
    entries: BTreeMap<(NodeTypeName, NodeLanguage), NodeRegistration>,
}
impl NodeRegistry {
    pub fn register(&mut self, registration: NodeRegistration) -> Result<(), RegistryError> {
        let key = (
            registration.descriptor.node_type().clone(),
            registration.language,
        );
        if self.entries.contains_key(&key) {
            return Err(RegistryError::DuplicateNode {
                node_type: key.0,
                language: key.1,
            });
        }
        self.entries.insert(key, registration);
        Ok(())
    }
    pub fn resolve(
        &self,
        node_type: &NodeTypeName,
        language: NodeLanguage,
    ) -> Result<&NodeRegistration, RegistryError> {
        self.entries
            .get(&(node_type.clone(), language))
            .ok_or_else(|| RegistryError::UnknownNode {
                node_type: node_type.clone(),
                language,
            })
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
