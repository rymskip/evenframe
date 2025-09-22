use crate::{
    schemasync::TableConfig,
    types::{StructConfig, TaggedUnion},
};

/// Trait for persistable structs (with ID field, representing database tables)
pub trait EvenframePersistableStruct {
    // Static method for registry and type-level operations
    fn static_table_config() -> TableConfig;

    // Instance method for runtime operations and polymorphism
    fn table_config(&self) -> TableConfig {
        Self::static_table_config()
    }
}

/// Trait for app structs (representing objects)
pub trait EvenframeAppStruct {
    fn struct_config() -> StructConfig;
}

/// Trait for tagged unions (representing enums)
pub trait EvenframeTaggedUnion {
    fn variants() -> TaggedUnion;
}

use serde::Deserializer;

pub trait EvenframeDeserialize<'de>: Sized {
    fn evenframe_deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>;
}
