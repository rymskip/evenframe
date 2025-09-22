use crate::{
    schemasync::TableConfig,
    types::{StructConfig, TaggedUnion},
};
use linkme::distributed_slice;
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Registry entry for persistable structs (tables)
#[derive(Clone, Copy)]
pub struct TableRegistryEntry {
    pub type_name: &'static str,
    pub table_config_fn: fn() -> TableConfig,
}

/// Registry entry for app structs (objects)
#[derive(Clone, Copy)]
pub struct ObjectRegistryEntry {
    pub type_name: &'static str,
    pub struct_config_fn: fn() -> StructConfig,
}

/// Registry entry for enums (tagged unions)
#[derive(Clone, Copy)]
pub struct EnumRegistryEntry {
    pub type_name: &'static str,
    pub tagged_union_fn: fn() -> TaggedUnion,
}

/// Distributed slice that collects table entries from all crates
#[distributed_slice]
pub static TABLE_REGISTRY_ENTRIES: [TableRegistryEntry] = [..];

/// Distributed slice that collects object entries from all crates
#[distributed_slice]
pub static OBJECT_REGISTRY_ENTRIES: [ObjectRegistryEntry] = [..];

/// Distributed slice that collects enum entries from all crates
#[distributed_slice]
pub static ENUM_REGISTRY_ENTRIES: [EnumRegistryEntry] = [..];

/// Runtime-accessible table registry
static TABLE_REGISTRY: Lazy<HashMap<&'static str, &'static TableRegistryEntry>> = Lazy::new(|| {
    TABLE_REGISTRY_ENTRIES
        .iter()
        .map(|entry| (entry.type_name, entry))
        .collect()
});

/// Runtime-accessible object registry
static OBJECT_REGISTRY: Lazy<HashMap<&'static str, &'static ObjectRegistryEntry>> =
    Lazy::new(|| {
        OBJECT_REGISTRY_ENTRIES
            .iter()
            .map(|entry| (entry.type_name, entry))
            .collect()
    });

/// Runtime-accessible enum registry
static ENUM_REGISTRY: Lazy<HashMap<&'static str, &'static EnumRegistryEntry>> = Lazy::new(|| {
    ENUM_REGISTRY_ENTRIES
        .iter()
        .map(|entry| (entry.type_name, entry))
        .collect()
});

/// Get table configuration by type name
pub fn get_table_config(type_name: &str) -> Option<TableConfig> {
    TABLE_REGISTRY
        .get(type_name)
        .map(|entry| (entry.table_config_fn)())
}

/// Get struct configuration by type name
pub fn get_struct_config(type_name: &str) -> Option<StructConfig> {
    OBJECT_REGISTRY
        .get(type_name)
        .map(|entry| (entry.struct_config_fn)())
}

/// Get tagged union by type name
pub fn get_tagged_union(type_name: &str) -> Option<TaggedUnion> {
    ENUM_REGISTRY
        .get(type_name)
        .map(|entry| (entry.tagged_union_fn)())
}

/// Type category for unified type resolution
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeCategory {
    Table,
    Object,
    Enum,
}

/// Resolve the category of a type by name
pub fn resolve_type_category(type_name: &str) -> Option<TypeCategory> {
    if TABLE_REGISTRY.contains_key(type_name) {
        Some(TypeCategory::Table)
    } else if OBJECT_REGISTRY.contains_key(type_name) {
        Some(TypeCategory::Object)
    } else if ENUM_REGISTRY.contains_key(type_name) {
        Some(TypeCategory::Enum)
    } else {
        None
    }
}

/// Get all registered table names
pub fn get_all_table_names() -> Vec<&'static str> {
    TABLE_REGISTRY.keys().copied().collect()
}

/// Get all registered object names
pub fn get_all_object_names() -> Vec<&'static str> {
    OBJECT_REGISTRY.keys().copied().collect()
}

/// Get all registered enum names
pub fn get_all_enum_names() -> Vec<&'static str> {
    ENUM_REGISTRY.keys().copied().collect()
}

/// Get all registered type names across all categories
pub fn get_all_type_names() -> HashMap<TypeCategory, Vec<&'static str>> {
    let mut result = HashMap::new();
    result.insert(TypeCategory::Table, get_all_table_names());
    result.insert(TypeCategory::Object, get_all_object_names());
    result.insert(TypeCategory::Enum, get_all_enum_names());
    result
}
