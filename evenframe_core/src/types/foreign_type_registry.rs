use crate::config::ForeignTypeConfig;
use std::collections::BTreeMap;

/// A registry that maps Rust type names to their foreign type configurations.
/// Built from the `[general.foreign_types]` config section.
#[derive(Debug, Clone, Default)]
pub struct ForeignTypeRegistry {
    /// Maps Rust type name (e.g., "DateTime", "chrono::DateTime") → canonical name
    name_to_canonical: BTreeMap<String, String>,
    /// Maps canonical name → config
    configs: BTreeMap<String, ForeignTypeConfig>,
}

impl ForeignTypeRegistry {
    /// Build a registry from the user's config.
    /// No built-in defaults — if foreign_types is empty, the registry is empty.
    pub fn from_config(foreign_types: &BTreeMap<String, ForeignTypeConfig>) -> Self {
        let mut registry = Self::default();

        for (canonical_name, config) in foreign_types {
            // Register all rust_type_names as aliases for this canonical name
            for rust_name in &config.rust_type_names {
                registry
                    .name_to_canonical
                    .insert(rust_name.clone(), canonical_name.clone());
            }
            // Also register the canonical name itself
            registry
                .name_to_canonical
                .insert(canonical_name.clone(), canonical_name.clone());
            registry
                .configs
                .insert(canonical_name.clone(), config.clone());
        }

        registry
    }

    /// Look up a foreign type by any of its Rust type names.
    /// Returns None if the name is not a configured foreign type.
    pub fn lookup(&self, rust_type_name: &str) -> Option<&ForeignTypeConfig> {
        self.name_to_canonical
            .get(rust_type_name)
            .and_then(|canonical| self.configs.get(canonical))
    }

    /// Check if a type name is a configured foreign type.
    pub fn is_foreign(&self, name: &str) -> bool {
        self.name_to_canonical.contains_key(name)
    }

    /// Return all configured foreign types.
    pub fn all(&self) -> &BTreeMap<String, ForeignTypeConfig> {
        &self.configs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_registry() {
        let registry = ForeignTypeRegistry::from_config(&BTreeMap::new());
        assert!(registry.lookup("DateTime").is_none());
        assert!(!registry.is_foreign("DateTime"));
    }

    #[test]
    fn test_lookup_by_canonical_name() {
        let mut foreign_types = BTreeMap::new();
        foreign_types.insert(
            "DateTime".to_string(),
            ForeignTypeConfig {
                rust_type_names: vec!["DateTime".to_string(), "chrono::DateTime".to_string()],
                surrealdb: "datetime".to_string(),
                arktype: "'string'".to_string(),
                ..Default::default()
            },
        );

        let registry = ForeignTypeRegistry::from_config(&foreign_types);
        let config = registry.lookup("DateTime").unwrap();
        assert_eq!(config.surrealdb, "datetime");
        assert_eq!(config.arktype, "'string'");
    }

    #[test]
    fn test_lookup_by_alias() {
        let mut foreign_types = BTreeMap::new();
        foreign_types.insert(
            "DateTime".to_string(),
            ForeignTypeConfig {
                rust_type_names: vec!["DateTime".to_string(), "chrono::DateTime".to_string()],
                surrealdb: "datetime".to_string(),
                ..Default::default()
            },
        );

        let registry = ForeignTypeRegistry::from_config(&foreign_types);
        let config = registry.lookup("chrono::DateTime").unwrap();
        assert_eq!(config.surrealdb, "datetime");
    }

    #[test]
    fn test_is_foreign() {
        let mut foreign_types = BTreeMap::new();
        foreign_types.insert(
            "Decimal".to_string(),
            ForeignTypeConfig {
                rust_type_names: vec!["Decimal".to_string()],
                ..Default::default()
            },
        );

        let registry = ForeignTypeRegistry::from_config(&foreign_types);
        assert!(registry.is_foreign("Decimal"));
        assert!(!registry.is_foreign("SomeOtherType"));
    }
}
