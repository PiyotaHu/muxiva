use std::{
    any::{type_name, Any, TypeId},
    collections::BTreeMap,
    error::Error,
    fmt,
    sync::{Arc, RwLock},
};

/// Stable graph-local name for one shared resource slot.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResourceKey(Box<str>);

impl ResourceKey {
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, ResourceStoreError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 255
            || value.trim() != value.as_ref()
            || value.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(ResourceStoreError::InvalidKey);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Structured graph-resource lookup failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceStoreError {
    InvalidKey,
    Missing {
        key: ResourceKey,
        requested: &'static str,
    },
    TypeMismatch {
        key: ResourceKey,
        requested: &'static str,
        stored: &'static str,
    },
    Stopped,
}

impl fmt::Display for ResourceStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey => formatter.write_str(
                "resource key must be non-empty, bounded, trimmed, and contain no ASCII controls",
            ),
            Self::Missing { key, requested } => {
                write!(
                    formatter,
                    "resource `{key}` is missing (requested `{requested}`)"
                )
            }
            Self::TypeMismatch {
                key,
                requested,
                stored,
            } => write!(
                formatter,
                "resource `{key}` stores `{stored}`, not requested `{requested}`"
            ),
            Self::Stopped => formatter.write_str("resource store is stopped"),
        }
    }
}

impl Error for ResourceStoreError {}

struct ResourceEntry {
    type_id: TypeId,
    type_name: &'static str,
    value: Arc<dyn Any + Send + Sync>,
}

#[derive(Default)]
struct ResourceState {
    stopped: bool,
    entries: BTreeMap<ResourceKey, ResourceEntry>,
}

/// Cloneable graph-level store backed by typed `Arc` ownership.
#[derive(Clone, Default)]
pub struct ResourceStore {
    state: Arc<RwLock<ResourceState>>,
}

impl ResourceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T>(&self, key: ResourceKey, value: Arc<T>) -> Result<(), ResourceStoreError>
    where
        T: Any + Send + Sync,
    {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if state.stopped {
            return Err(ResourceStoreError::Stopped);
        }
        state.entries.insert(
            key,
            ResourceEntry {
                type_id: TypeId::of::<T>(),
                type_name: type_name::<T>(),
                value,
            },
        );
        Ok(())
    }

    pub fn get<T>(&self, key: &ResourceKey) -> Result<Arc<T>, ResourceStoreError>
    where
        T: Any + Send + Sync,
    {
        let state = self.state.read().unwrap_or_else(|error| error.into_inner());
        let entry = state
            .entries
            .get(key)
            .ok_or_else(|| ResourceStoreError::Missing {
                key: key.clone(),
                requested: type_name::<T>(),
            })?;
        if entry.type_id != TypeId::of::<T>() {
            return Err(ResourceStoreError::TypeMismatch {
                key: key.clone(),
                requested: type_name::<T>(),
                stored: entry.type_name,
            });
        }
        Arc::downcast::<T>(entry.value.clone()).map_err(|_| ResourceStoreError::TypeMismatch {
            key: key.clone(),
            requested: type_name::<T>(),
            stored: entry.type_name,
        })
    }

    pub fn stop(&self) -> bool {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let first = !state.stopped;
        state.stopped = true;
        state.entries.clear();
        first
    }

    /// Rejects new inserts while retaining existing resources for lifecycle cleanup.
    pub fn seal(&self) -> bool {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let first = !state.stopped;
        state.stopped = true;
        first
    }

    pub fn is_stopped(&self) -> bool {
        self.state
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .stopped
    }
}
