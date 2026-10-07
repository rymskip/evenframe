//! SurrealDB Type Mapper Implementation
//!
//! Maps Evenframe's FieldType to SurrealDB native types.

use crate::schemasync::TableConfig;
use crate::types::{
    FieldType, ForeignTypeRegistry, StructConfig, TaggedUnion, record_link_target_surql,
};
use std::collections::BTreeMap;

/// Type mapper for SurrealDB
pub struct SurrealdbTypeMapper<'a> {
    registry: &'a ForeignTypeRegistry,
    option_none: crate::schemasync::config::OptionNone,
    registries: Option<Registries<'a>>,
}

/// The types a `RecordLink` target resolves against.
struct Registries<'a> {
    structs: &'a BTreeMap<String, StructConfig>,
    tables: &'a BTreeMap<String, TableConfig>,
    enums: &'a BTreeMap<String, TaggedUnion>,
}

impl<'a> SurrealdbTypeMapper<'a> {
    pub fn new(registry: &'a ForeignTypeRegistry) -> Self {
        Self {
            registry,
            option_none: crate::schemasync::config::OptionNone::default(),
            registries: None,
        }
    }

    pub fn with_option_none(mut self, option_none: crate::schemasync::config::OptionNone) -> Self {
        self.option_none = option_none;
        self
    }

    /// Provide the struct, table and enum registries so `RecordLink` targets
    /// resolve like [`record_link_target_surql`]: a projection to its
    /// underlying table, a union to every table it spans. When unset,
    /// `record<X>` references emit `X` verbatim.
    pub fn with_registries(
        mut self,
        structs: &'a BTreeMap<String, StructConfig>,
        tables: &'a BTreeMap<String, TableConfig>,
        enums: &'a BTreeMap<String, TaggedUnion>,
    ) -> Self {
        self.registries = Some(Registries {
            structs,
            tables,
            enums,
        });
        self
    }

    /// Map a FieldType to SurrealQL type syntax
    pub fn field_type_to_surql(&self, field_type: &FieldType) -> String {
        self.field_type_to_surql_inner(field_type)
    }

    fn resolve_record_link_target(&self, name: &str) -> Option<String> {
        let registries = self.registries.as_ref()?;
        record_link_target_surql(
            name,
            registries.tables,
            registries.structs,
            registries.enums,
        )
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
            FieldType::Duration => "duration".to_string(),
            FieldType::FromText(kind) => kind.surql_type().to_string(),
            FieldType::IsoDate | FieldType::EpochMillis => "datetime".to_string(),
            FieldType::JsonText(inner) => self.field_type_to_surql_inner(inner),
            FieldType::Option(inner) => self
                .option_none
                .surql_type(&self.field_type_to_surql_inner(inner)),
            FieldType::Vec(inner) => {
                format!("array<{}>", self.field_type_to_surql_inner(inner))
            }
            FieldType::Tuple(_) => {
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

#[cfg(test)]
mod tests {
    use super::{
        BTreeMap, FieldType, ForeignTypeRegistry, StructConfig, SurrealdbTypeMapper, TableConfig,
        TaggedUnion,
    };

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

        let enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();
        let mapper = SurrealdbTypeMapper::new(&registry).with_registries(&structs, &tables, &enums);

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

        let enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();
        let mapper = SurrealdbTypeMapper::new(&registry).with_registries(&structs, &tables, &enums);

        assert_eq!(
            mapper.field_type_to_surql(&vec_record("AliasedTable")),
            "array<record<real_table>>"
        );
    }

    #[test]
    fn record_link_falls_through_to_literal_when_name_unknown() {
        let structs: BTreeMap<String, StructConfig> = BTreeMap::new();
        let tables: BTreeMap<String, TableConfig> = BTreeMap::new();
        let enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();
        let mapper = SurrealdbTypeMapper::new(&registry).with_registries(&structs, &tables, &enums);
        assert_eq!(
            mapper.field_type_to_surql(&vec_record("UnknownThing")),
            "array<record<UnknownThing>>"
        );
    }
}
