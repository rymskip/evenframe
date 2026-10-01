#[cfg(feature = "metadata")]
use crate::{
    schemasync::TableConfig,
    types::{StructConfig, TaggedUnion},
};
use serde::Deserializer;

/// A database table: a struct with an `id` field, or a union of tables. What
/// a `RecordLink` can point at.
pub trait EvenframeTable {}

/// Trait for persistable structs (with ID field, representing database tables)
#[cfg(feature = "metadata")]
pub trait EvenframePersistableStruct {
    // Static method for registry and type-level operations
    fn static_table_config() -> TableConfig;

    // Instance method for runtime operations and polymorphism
    fn table_config(&self) -> TableConfig {
        Self::static_table_config()
    }
}

/// Trait for app structs (representing objects)
#[cfg(feature = "metadata")]
pub trait EvenframeAppStruct {
    fn struct_config() -> StructConfig;
}

/// Trait for tagged unions (representing enums)
#[cfg(feature = "metadata")]
pub trait EvenframeTaggedUnion {
    fn variants() -> TaggedUnion;
}

pub trait EvenframeDeserialize<'de>: Sized {
    fn evenframe_deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>;
}
