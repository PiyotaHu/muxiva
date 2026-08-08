use std::{collections::BTreeSet, fmt};

use crate::{
    ErrorCategory, MuxivaError, NamespacedName, NodeId, ProducerId, Result, SchemaVersion, Value,
};

/// Determines whether an extension is included in public diagnostic views.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionVisibility {
    /// The extension may be included in public diagnostic views.
    Public,
    /// The extension is omitted from public diagnostic views and default logs.
    Private,
}

/// Identifies who produced an extension.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExtensionProducer {
    /// An extension produced by Muxiva core.
    Core,
    /// An extension produced by a graph node.
    Node(NodeId),
    /// An extension produced outside the graph.
    External(ProducerId),
}

/// An immutable, versioned extension record.
#[derive(Clone, Eq, PartialEq)]
pub struct Extension {
    key: NamespacedName,
    schema_version: SchemaVersion,
    producer: ExtensionProducer,
    visibility: ExtensionVisibility,
    value: Value,
}

impl Extension {
    /// Creates an immutable extension record.
    pub fn new(
        key: NamespacedName,
        schema_version: SchemaVersion,
        producer: ExtensionProducer,
        visibility: ExtensionVisibility,
        value: Value,
    ) -> Self {
        Self {
            key,
            schema_version,
            producer,
            visibility,
            value,
        }
    }

    /// Returns this extension's qualified key.
    pub fn key(&self) -> &NamespacedName {
        &self.key
    }

    /// Returns this extension's schema version.
    pub const fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// Returns the producer of this extension.
    pub fn producer(&self) -> &ExtensionProducer {
        &self.producer
    }

    /// Returns this extension's visibility.
    pub const fn visibility(&self) -> ExtensionVisibility {
        self.visibility
    }

    /// Returns this extension's immutable value.
    pub fn value(&self) -> &Value {
        &self.value
    }
}

impl fmt::Debug for Extension {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("Extension");
        match self.visibility {
            ExtensionVisibility::Public => {
                debug.field("key", &self.key);
            }
            ExtensionVisibility::Private => {
                debug.field("key", &"<private>");
            }
        }
        debug
            .field("schema_version", &self.schema_version)
            .field("producer", &self.producer)
            .field("visibility", &self.visibility)
            .finish()
    }
}

/// Immutable extension records in caller-provided order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Extensions(Box<[Extension]>);

impl Extensions {
    /// Creates an empty extension collection.
    pub fn empty() -> Self {
        Self(Box::new([]))
    }

    /// Creates extensions after rejecting duplicate key and schema-version pairs.
    pub fn try_from_iter<I>(extensions: I) -> Result<Self>
    where
        I: IntoIterator<Item = Extension>,
    {
        let mut keys = BTreeSet::new();
        let mut records = Vec::new();

        for extension in extensions {
            let key = (extension.key.clone(), extension.schema_version);
            if !keys.insert(key) {
                return Err(MuxivaError::new(
                    ErrorCategory::Validation,
                    "MUXIVA-FRM-EXTENSION-DUPLICATE",
                    "extension key and schema version must be unique",
                ));
            }
            records.push(extension);
        }

        Ok(Self(records.into_boxed_slice()))
    }

    /// Returns an extension identified by its key and schema version.
    pub fn get(&self, key: &NamespacedName, version: SchemaVersion) -> Option<&Extension> {
        self.0
            .iter()
            .find(|extension| extension.key == *key && extension.schema_version == version)
    }

    /// Iterates over every extension in input order.
    pub fn iter(&self) -> impl Iterator<Item = &Extension> {
        self.0.iter()
    }

    /// Iterates over public extensions in input order.
    pub fn public_iter(&self) -> impl Iterator<Item = &Extension> {
        self.0
            .iter()
            .filter(|extension| extension.visibility == ExtensionVisibility::Public)
    }

    /// Returns the number of extensions.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether there are no extensions.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Extension, ExtensionProducer, ExtensionVisibility, Extensions};
    use crate::{NamespacedName, SchemaVersion, Value};

    fn extension(key: &str, version: u32, visibility: ExtensionVisibility) -> Extension {
        Extension::new(
            NamespacedName::new(key).unwrap(),
            SchemaVersion::new(version).unwrap(),
            ExtensionProducer::Core,
            visibility,
            Value::String(Box::from("private value")),
        )
    }

    #[test]
    fn extensions_preserve_order_and_filter_visibility() {
        let extensions = Extensions::try_from_iter([
            extension("com.example.public", 1, ExtensionVisibility::Public),
            extension("com.example.private", 1, ExtensionVisibility::Private),
        ])
        .unwrap();

        assert_eq!(extensions.iter().count(), 2);
        assert_eq!(extensions.public_iter().count(), 1);
        assert_eq!(
            extensions
                .iter()
                .map(|extension| extension.key().as_str())
                .collect::<Vec<_>>(),
            ["com.example.public", "com.example.private"]
        );
    }

    #[test]
    fn extensions_reject_duplicate_key_and_version_but_allow_migration_versions() {
        let error = Extensions::try_from_iter([
            extension("com.example.trace", 1, ExtensionVisibility::Public),
            extension("com.example.trace", 1, ExtensionVisibility::Private),
        ])
        .unwrap_err();
        assert_eq!(error.code(), "MUXIVA-FRM-EXTENSION-DUPLICATE");

        let extensions = Extensions::try_from_iter([
            extension("com.example.trace", 1, ExtensionVisibility::Public),
            extension("com.example.trace", 2, ExtensionVisibility::Private),
        ])
        .unwrap();
        assert_eq!(extensions.len(), 2);
    }

    #[test]
    fn extension_debug_omits_values_and_redacts_private_keys() {
        let public = extension("com.example.public", 1, ExtensionVisibility::Public);
        let private = extension("com.example.private", 1, ExtensionVisibility::Private);

        let public_debug = format!("{public:?}");
        let private_debug = format!("{private:?}");
        assert!(public_debug.contains("com.example.public"));
        assert!(!public_debug.contains("private value"));
        assert!(private_debug.contains("<private>"));
        assert!(!private_debug.contains("com.example.private"));
        assert!(!private_debug.contains("private value"));
    }
}
