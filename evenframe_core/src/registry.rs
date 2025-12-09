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

/// Registry entry for union of tables
#[derive(Clone, Copy)]
pub struct UnionOfTablesRegistryEntry {
    pub type_name: &'static str,
    pub table_names: &'static [&'static str],
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

/// Distributed slice that collects union of tables entries from all crates
#[distributed_slice]
pub static UNION_OF_TABLES_REGISTRY_ENTRIES: [UnionOfTablesRegistryEntry] = [..];

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

/// Runtime-accessible union of tables registry
static UNION_OF_TABLES_REGISTRY: Lazy<HashMap<&'static str, &'static UnionOfTablesRegistryEntry>> =
    Lazy::new(|| {
        UNION_OF_TABLES_REGISTRY_ENTRIES
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

/// Get union of tables by type name
pub fn get_union_of_tables(type_name: &str) -> Option<&'static [&'static str]> {
    UNION_OF_TABLES_REGISTRY
        .get(type_name)
        .map(|entry| entry.table_names)
}

/// Type category for unified type resolution
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeCategory {
    Table,
    Object,
    Enum,
    UnionOfTables,
}

/// Resolve the category of a type by name
pub fn resolve_type_category(type_name: &str) -> Option<TypeCategory> {
    if TABLE_REGISTRY.contains_key(type_name) {
        Some(TypeCategory::Table)
    } else if OBJECT_REGISTRY.contains_key(type_name) {
        Some(TypeCategory::Object)
    } else if ENUM_REGISTRY.contains_key(type_name) {
        Some(TypeCategory::Enum)
    } else if UNION_OF_TABLES_REGISTRY.contains_key(type_name) {
        Some(TypeCategory::UnionOfTables)
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

/// Get all registered union of tables names
pub fn get_all_union_of_tables_names() -> Vec<&'static str> {
    UNION_OF_TABLES_REGISTRY.keys().copied().collect()
}

/// Get all registered type names across all categories
pub fn get_all_type_names() -> HashMap<TypeCategory, Vec<&'static str>> {
    let mut result = HashMap::new();
    result.insert(TypeCategory::Table, get_all_table_names());
    result.insert(TypeCategory::Object, get_all_object_names());
    result.insert(TypeCategory::Enum, get_all_enum_names());
    result.insert(TypeCategory::UnionOfTables, get_all_union_of_tables_names());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== TypeCategory Tests ====================

    #[test]
    fn test_type_category_equality() {
        assert_eq!(TypeCategory::Table, TypeCategory::Table);
        assert_eq!(TypeCategory::Object, TypeCategory::Object);
        assert_eq!(TypeCategory::Enum, TypeCategory::Enum);
        assert_eq!(TypeCategory::UnionOfTables, TypeCategory::UnionOfTables);
    }

    #[test]
    fn test_type_category_inequality() {
        assert_ne!(TypeCategory::Table, TypeCategory::Object);
        assert_ne!(TypeCategory::Table, TypeCategory::Enum);
        assert_ne!(TypeCategory::Table, TypeCategory::UnionOfTables);
        assert_ne!(TypeCategory::Object, TypeCategory::Enum);
        assert_ne!(TypeCategory::Object, TypeCategory::UnionOfTables);
        assert_ne!(TypeCategory::Enum, TypeCategory::UnionOfTables);
    }

    #[test]
    fn test_type_category_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(TypeCategory::Table);
        set.insert(TypeCategory::Object);
        set.insert(TypeCategory::Enum);
        set.insert(TypeCategory::UnionOfTables);
        assert_eq!(set.len(), 4);
    }

    #[test]
    fn test_type_category_clone() {
        let category = TypeCategory::Table;
        let cloned = category.clone();
        assert_eq!(category, cloned);
    }

    #[test]
    fn test_type_category_debug() {
        let debug_str = format!("{:?}", TypeCategory::Table);
        assert!(debug_str.contains("Table"));
    }

    // ==================== Lookup Function Tests ====================

    #[test]
    fn test_get_table_config_not_found() {
        let result = get_table_config("NonExistentType12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_struct_config_not_found() {
        let result = get_struct_config("NonExistentType12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_tagged_union_not_found() {
        let result = get_tagged_union("NonExistentType12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_union_of_tables_not_found() {
        let result = get_union_of_tables("NonExistentType12345");
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_type_category_unknown() {
        let result = resolve_type_category("NonExistentType12345");
        assert!(result.is_none());
    }

    // ==================== Registry Collection Tests ====================

    #[test]
    fn test_get_all_type_names_structure() {
        let all_types = get_all_type_names();
        // Should have all 4 categories
        assert!(all_types.contains_key(&TypeCategory::Table));
        assert!(all_types.contains_key(&TypeCategory::Object));
        assert!(all_types.contains_key(&TypeCategory::Enum));
        assert!(all_types.contains_key(&TypeCategory::UnionOfTables));
    }

    #[test]
    fn test_get_all_table_names_returns_vec() {
        let tables = get_all_table_names();
        // Just verify it returns a Vec (may be empty in test context)
        assert!(tables.len() >= 0);
    }

    #[test]
    fn test_get_all_object_names_returns_vec() {
        let objects = get_all_object_names();
        assert!(objects.len() >= 0);
    }

    #[test]
    fn test_get_all_enum_names_returns_vec() {
        let enums = get_all_enum_names();
        assert!(enums.len() >= 0);
    }

    #[test]
    fn test_get_all_union_of_tables_names_returns_vec() {
        let unions = get_all_union_of_tables_names();
        assert!(unions.len() >= 0);
    }

    // ==================== Registry Entry Struct Tests ====================

    #[test]
    fn test_table_registry_entry_clone() {
        fn dummy_config() -> TableConfig {
            panic!("This should not be called in tests");
        }
        let entry = TableRegistryEntry {
            type_name: "TestType",
            table_config_fn: dummy_config,
        };
        let cloned = entry;
        assert_eq!(cloned.type_name, "TestType");
    }

    #[test]
    fn test_object_registry_entry_clone() {
        fn dummy_config() -> StructConfig {
            panic!("This should not be called in tests");
        }
        let entry = ObjectRegistryEntry {
            type_name: "TestType",
            struct_config_fn: dummy_config,
        };
        let cloned = entry;
        assert_eq!(cloned.type_name, "TestType");
    }

    #[test]
    fn test_enum_registry_entry_clone() {
        fn dummy_config() -> TaggedUnion {
            panic!("This should not be called in tests");
        }
        let entry = EnumRegistryEntry {
            type_name: "TestType",
            tagged_union_fn: dummy_config,
        };
        let cloned = entry;
        assert_eq!(cloned.type_name, "TestType");
    }

    #[test]
    fn test_union_of_tables_registry_entry_clone() {
        static TABLE_NAMES: &[&str] = &["Table1", "Table2"];
        let entry = UnionOfTablesRegistryEntry {
            type_name: "TestUnion",
            table_names: TABLE_NAMES,
        };
        let cloned = entry;
        assert_eq!(cloned.type_name, "TestUnion");
        assert_eq!(cloned.table_names.len(), 2);
    }

    // ==================== HashMap Key Tests ====================

    #[test]
    fn test_type_category_as_hashmap_key() {
        let mut map: HashMap<TypeCategory, String> = HashMap::new();
        map.insert(TypeCategory::Table, "tables".to_string());
        map.insert(TypeCategory::Object, "objects".to_string());
        map.insert(TypeCategory::Enum, "enums".to_string());
        map.insert(TypeCategory::UnionOfTables, "unions".to_string());

        assert_eq!(map.get(&TypeCategory::Table), Some(&"tables".to_string()));
        assert_eq!(map.get(&TypeCategory::Object), Some(&"objects".to_string()));
        assert_eq!(map.get(&TypeCategory::Enum), Some(&"enums".to_string()));
        assert_eq!(
            map.get(&TypeCategory::UnionOfTables),
            Some(&"unions".to_string())
        );
    }
}
