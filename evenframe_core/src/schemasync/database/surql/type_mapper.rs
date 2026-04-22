//! SurrealDB Type Mapper Implementation
//!
//! Maps Evenframe's FieldType to SurrealDB native types.

use crate::schemasync::TableConfig;
use crate::schemasync::database::types::mapper::TypeMapper;
use crate::types::{FieldType, ForeignTypeRegistry, StructConfig};
use convert_case::{Case, Casing};
use std::collections::BTreeMap;

use super::value::to_surreal_string;

/// Type mapper for SurrealDB
pub struct SurrealdbTypeMapper<'a> {
    registry: &'a ForeignTypeRegistry,
    structs: Option<&'a BTreeMap<String, StructConfig>>,
    tables: Option<&'a BTreeMap<String, TableConfig>>,
}

impl<'a> SurrealdbTypeMapper<'a> {
    pub fn new(registry: &'a ForeignTypeRegistry) -> Self {
        Self {
            registry,
            structs: None,
            tables: None,
        }
    }

    /// Provide struct/table registries so the mapper can resolve
    /// `output_override` on `RecordLink` targets — e.g. a synthetic
    /// projection like `PartialUser` whose override points to the underlying
    /// `User` table. When unset, `record<X>` references emit `X` verbatim.
    pub fn with_struct_table_registries(
        mut self,
        structs: &'a BTreeMap<String, StructConfig>,
        tables: &'a BTreeMap<String, TableConfig>,
    ) -> Self {
        self.structs = Some(structs);
        self.tables = Some(tables);
        self
    }

    /// Map a FieldType to SurrealQL type syntax
    pub fn field_type_to_surql(&self, field_type: &FieldType) -> String {
        self.field_type_to_surql_inner(field_type)
    }

    /// Resolve a `RecordLink` target name through `output_override`. Tries the
    /// struct registry first (PascalCase keys), then the table registry
    /// (snake_case keys). Returns `None` when neither registry is supplied or
    /// the name doesn't match any known type.
    fn resolve_record_link_target(&self, name: &str) -> Option<String> {
        if let Some(structs) = self.structs
            && let Some(sc) = structs.get(name)
        {
            return Some(sc.effective().struct_name.to_case(Case::Snake));
        }
        if let Some(tables) = self.tables
            && let Some(tc) = tables.get(&name.to_case(Case::Snake))
        {
            return Some(tc.effective().table_name.clone());
        }
        None
    }

    fn field_type_to_surql_inner(&self, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String => "string".to_string(),
            FieldType::Char => "string".to_string(),
            FieldType::Bool => "bool".to_string(),
            FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 | FieldType::I128 => {
                "int".to_string()
            }
            FieldType::Isize => "int".to_string(),
            FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 | FieldType::U128 => {
                "int".to_string()
            }
            FieldType::Usize => "int".to_string(),
            FieldType::F32 | FieldType::F64 => "float".to_string(),
            FieldType::Unit => "null".to_string(),
            FieldType::Option(inner) => {
                format!("option<{}>", self.field_type_to_surql_inner(inner))
            }
            FieldType::Vec(inner) => {
                format!("array<{}>", self.field_type_to_surql_inner(inner))
            }
            FieldType::Tuple(_types) => {
                // SurrealDB doesn't have tuple types, use array<any>
                "array<any>".to_string()
            }
            FieldType::Struct(_) => "object".to_string(),
            FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) => "object".to_string(),
            FieldType::RecordLink(inner) => {
                // Try to extract the table name from the inner type
                if let FieldType::Other(table_name) = inner.as_ref() {
                    let resolved = self
                        .resolve_record_link_target(table_name)
                        .unwrap_or_else(|| table_name.clone());
                    format!("record<{}>", resolved)
                } else {
                    "record".to_string()
                }
            }
            FieldType::Other(name) => {
                if let Some(ftc) = self.registry.lookup(name) {
                    ftc.surrealdb.clone()
                } else {
                    name.clone()
                }
            }
        }
    }
}

impl<'a> TypeMapper for SurrealdbTypeMapper<'a> {
    fn field_type_to_native(&self, field_type: &FieldType) -> String {
        self.field_type_to_surql(field_type)
    }

    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String {
        to_surreal_string(field_type, value, self.registry)
    }

    fn supports_native_arrays(&self) -> bool {
        true // SurrealDB has native array<T> type
    }

    fn supports_jsonb(&self) -> bool {
        false // SurrealDB uses 'object' type, not JSONB
    }

    fn supports_native_enums(&self) -> bool {
        false // SurrealDB doesn't have CREATE TYPE ENUM
    }

    fn supports_interval(&self) -> bool {
        true // SurrealDB has native duration type
    }

    fn quote_char(&self) -> char {
        '`' // SurrealDB uses backticks for identifiers
    }

    fn format_datetime(&self, value: &str) -> String {
        format!("d'{}'", value)
    }

    fn format_duration(&self, nanos: i64) -> String {
        format!("duration::from_nanos({})", nanos)
    }

    fn format_array(&self, field_type: &FieldType, values: &[serde_json::Value]) -> String {
        let inner_type = if let FieldType::Vec(inner) = field_type {
            inner.as_ref()
        } else {
            &FieldType::String
        };

        let formatted: Vec<String> = values
            .iter()
            .map(|v| self.format_value(inner_type, v))
            .collect();

        format!("[{}]", formatted.join(", "))
    }

    fn auto_increment_type(&self) -> &'static str {
        "record" // SurrealDB auto-generates record IDs
    }

    fn uuid_type(&self) -> &'static str {
        "string" // UUIDs are stored as strings in SurrealDB
    }

    fn uuid_generate_expr(&self) -> Option<&'static str> {
        Some("rand::uuid::v4()") // SurrealDB function for UUID generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_record(name: &str) -> FieldType {
        FieldType::Vec(Box::new(FieldType::RecordLink(Box::new(FieldType::Other(
            name.to_string(),
        )))))
    }

    #[test]
    fn record_link_emits_literal_when_no_registries_supplied() {
        let registry = ForeignTypeRegistry::default();
        let mapper = SurrealdbTypeMapper::new(&registry);
        assert_eq!(
            mapper.field_type_to_surql(&vec_record("PartialUser")),
            "array<record<PartialUser>>"
        );
    }

    #[test]
    fn record_link_resolves_struct_override_to_underlying_table() {
        // PartialUser → User; User is a real table.
        let partial_user = StructConfig {
            struct_name: "PartialUser".to_string(),
            output_override: Some(Box::new(StructConfig {
                struct_name: "User".to_string(),
                ..StructConfig::default()
            })),
            ..StructConfig::default()
        };
        let mut structs = BTreeMap::new();
        structs.insert("PartialUser".to_string(), partial_user);
        let tables: BTreeMap<String, TableConfig> = BTreeMap::new();

        let registry = ForeignTypeRegistry::default();
        let mapper =
            SurrealdbTypeMapper::new(&registry).with_struct_table_registries(&structs, &tables);

        assert_eq!(
            mapper.field_type_to_surql(&vec_record("PartialUser")),
            "array<record<user>>"
        );
    }

    #[test]
    fn record_link_resolves_table_override_to_alias_table_name() {
        let aliased_table = TableConfig {
            table_name: "aliased_table".to_string(),
            struct_config: StructConfig {
                struct_name: "AliasedTable".to_string(),
                ..StructConfig::default()
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![],
            indexes: vec![],
            output_override: Some(Box::new(TableConfig {
                table_name: "real_table".to_string(),
                struct_config: StructConfig {
                    struct_name: "RealTable".to_string(),
                    ..StructConfig::default()
                },
                relation: None,
                permissions: None,
                mock_generation_config: None,
                events: vec![],
                indexes: vec![],
                output_override: None,
            })),
        };
        let structs: BTreeMap<String, StructConfig> = BTreeMap::new();
        let mut tables = BTreeMap::new();
        tables.insert("aliased_table".to_string(), aliased_table);

        let registry = ForeignTypeRegistry::default();
        let mapper =
            SurrealdbTypeMapper::new(&registry).with_struct_table_registries(&structs, &tables);

        assert_eq!(
            mapper.field_type_to_surql(&vec_record("AliasedTable")),
            "array<record<real_table>>"
        );
    }

    #[test]
    fn record_link_falls_through_to_literal_when_name_unknown() {
        let structs: BTreeMap<String, StructConfig> = BTreeMap::new();
        let tables: BTreeMap<String, TableConfig> = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();
        let mapper =
            SurrealdbTypeMapper::new(&registry).with_struct_table_registries(&structs, &tables);
        assert_eq!(
            mapper.field_type_to_surql(&vec_record("UnknownThing")),
            "array<record<UnknownThing>>"
        );
    }
}
