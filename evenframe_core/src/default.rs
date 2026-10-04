use crate::schemasync::table::surql_ident;
use crate::types::{EnumRepresentation, FieldType, TaggedUnion, VariantData};
use crate::types::{StructConfig, StructField};
use convert_case::{Case, Casing};
use std::collections::BTreeMap;
use surrealdb_types::ToSql;
use tracing::trace;

/// The SurrealQL zero value for a field type, used as its `DEFAULT`. `None`
/// when the type has no valid zero value: a required record link cannot
/// point at nothing, and neither can a struct or variant that contains one.
pub fn field_type_to_surql_default(
    field_name: &String,
    table_name: &String,
    field_type: &FieldType,
    enums: &BTreeMap<String, TaggedUnion>,
    app_structs: &BTreeMap<String, StructConfig>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Option<String> {
    trace!(
        "Generating SURQL default for field '{}' in table '{}', type: {:?}",
        field_name, table_name, field_type
    );
    let default_of = |ty: &FieldType| {
        field_type_to_surql_default(field_name, table_name, ty, enums, app_structs, registry)
    };
    let result = match field_type {
        FieldType::String | FieldType::Char => Some("''".to_string()),
        FieldType::Bool => Some("false".to_string()),
        FieldType::Unit | FieldType::Option(_) => Some("NULL".to_string()),
        FieldType::F32 | FieldType::F64 => Some("0.0f".to_string()),
        FieldType::I8
        | FieldType::I16
        | FieldType::I32
        | FieldType::I64
        | FieldType::I128
        | FieldType::Isize
        | FieldType::U8
        | FieldType::U16
        | FieldType::U32
        | FieldType::U64
        | FieldType::U128
        | FieldType::Usize => Some("0".to_string()),
        FieldType::Duration => Some(surrealdb_types::Duration::default().to_sql()),
        FieldType::Tuple(inner_types) => inner_types
            .iter()
            .map(default_of)
            .collect::<Option<Vec<_>>>()
            .map(|defaults| format!("[{}]", defaults.join(", "))),
        FieldType::Struct(fields) => fields
            .iter()
            .map(|(name, ftype)| {
                default_of(ftype).map(|value| format!("{}: {}", surql_ident(name), value))
            })
            .collect::<Option<Vec<_>>>()
            .map(|fields| format!("{{ {} }}", fields.join(", "))),
        FieldType::Vec(_) => Some("[]".to_string()),
        FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) => Some("{}".to_string()),
        FieldType::RecordLink(_) => None,
        FieldType::Other(name) => {
            if let Some(ftc) = registry.lookup(name) {
                Some(ftc.default_value_surql.clone())
            } else if let Some(enum_schema) = enums.values().find(|e| e.enum_name == *name) {
                enum_surql_default(
                    enum_schema,
                    field_name,
                    table_name,
                    enums,
                    app_structs,
                    registry,
                )
            } else if let Some(struct_config) = app_structs.values().find(|struct_config| {
                struct_config.struct_name.to_case(Case::Pascal) == name.to_case(Case::Pascal)
            }) {
                struct_fields_to_surql_default_object(
                    &struct_config.fields,
                    table_name,
                    enums,
                    app_structs,
                    registry,
                )
            } else {
                // A table reference or an unresolved type has no zero value.
                None
            }
        }
    };
    trace!("Generated SURQL default: {:?}", result);
    result
}

/// The default of an enum: its `#[default]` variant, else the first declared.
fn enum_surql_default(
    enum_schema: &TaggedUnion,
    field_name: &String,
    table_name: &String,
    enums: &BTreeMap<String, TaggedUnion>,
    app_structs: &BTreeMap<String, StructConfig>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Option<String> {
    let chosen_variant = enum_schema
        .variants
        .iter()
        .find(|v| v.is_default)
        .or_else(|| enum_schema.variants.first())?;
    let Some(variant_data) = &chosen_variant.data else {
        return Some(match &enum_schema.representation {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                format!("{{ {}: '{}' }}", surql_ident(tag), chosen_variant.db_name())
            }
            _ => format!("'{}'", chosen_variant.db_name()),
        });
    };
    let inner_default = match variant_data {
        // An inline payload is anonymous: it is never registered in
        // `app_structs`, so build the object from its own fields.
        VariantData::InlineStruct(enum_struct) => struct_fields_to_surql_default_object(
            &enum_struct.fields,
            table_name,
            enums,
            app_structs,
            registry,
        )?,
        VariantData::DataStructureRef(field_type) => field_type_to_surql_default(
            field_name,
            table_name,
            field_type,
            enums,
            app_structs,
            registry,
        )?,
    };
    Some(match &enum_schema.representation {
        EnumRepresentation::ExternallyTagged => {
            format!(
                "{{ {}: {} }}",
                surql_ident(chosen_variant.db_name()),
                inner_default
            )
        }
        EnumRepresentation::InternallyTagged { tag } => {
            if let VariantData::InlineStruct(_) = variant_data {
                let trimmed = inner_default.trim();
                if trimmed.starts_with('{') && trimmed.ends_with('}') {
                    let inner = &trimmed[1..trimmed.len() - 1];
                    format!(
                        "{{ {}: '{}', {} }}",
                        surql_ident(tag),
                        chosen_variant.db_name(),
                        inner.trim()
                    )
                } else {
                    format!("{{ {}: '{}' }}", surql_ident(tag), chosen_variant.db_name())
                }
            } else {
                format!(
                    "{{ {}: {} }}",
                    surql_ident(chosen_variant.db_name()),
                    inner_default
                )
            }
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!(
                "{{ {}: '{}', {}: {} }}",
                surql_ident(tag),
                chosen_variant.db_name(),
                surql_ident(content),
                inner_default
            )
        }
        EnumRepresentation::Untagged => inner_default,
    })
}

/// Build the `{ field: default, ... }` object literal for a struct's fields:
/// each subfield's `define_config.default` when present, the type-derived
/// fallback otherwise. Used for both registered embedded structs and inline
/// enum-variant payloads.
fn struct_fields_to_surql_default_object(
    fields: &[StructField],
    table_name: &String,
    enums: &BTreeMap<String, TaggedUnion>,
    app_structs: &BTreeMap<String, StructConfig>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Option<String> {
    fields
        .iter()
        .map(|table_field| {
            let table_field = table_field.effective();
            let value = match table_field
                .define_config
                .as_ref()
                .and_then(|dc| dc.default.as_deref())
            {
                Some(default) => default.to_string(),
                None => field_type_to_surql_default(
                    &table_field.field_name,
                    table_name,
                    &table_field.field_type,
                    enums,
                    app_structs,
                    registry,
                )?,
            };
            Some(format!("{}: {}", surql_ident(table_field.db_name()), value))
        })
        .collect::<Option<Vec<_>>>()
        .map(|fields| format!("{{ {} }}", fields.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::{FieldType, VariantData, field_type_to_surql_default};
    use crate::schemasync::DefineConfig;
    use crate::types::{
        EnumRepresentation, ForeignTypeRegistry, StructConfig, StructField, TaggedUnion, Variant,
    };
    use std::collections::BTreeMap;

    #[test]
    fn an_embedded_struct_defaults_under_its_database_keys() {
        let registry = ForeignTypeRegistry::default();
        let field = |name: &str, surreal: Option<&str>| StructField {
            field_name: name.to_owned(),
            field_type: FieldType::String,
            wire: crate::types::Wire {
                surreal: surreal.map(str::to_owned),
                ..Default::default()
            },
            ..Default::default()
        };
        let address = StructConfig {
            struct_name: "Address".to_owned(),
            fields: vec![field("zipCode", None), field("city_name", Some("cityName"))],
            ..Default::default()
        };
        let default = field_type_to_surql_default(
            &"address".to_owned(),
            &"person".to_owned(),
            &FieldType::Other("Address".to_owned()),
            &BTreeMap::new(),
            &BTreeMap::from([("Address".to_owned(), address)]),
            &registry,
        );
        assert_eq!(default.as_deref(), Some("{ zipCode: '', cityName: '' }"));
    }

    #[test]
    fn a_duration_defaults_to_zero() {
        let registry = ForeignTypeRegistry::default();
        let (field, table) = ("limit".to_string(), "timer".to_string());
        let default = field_type_to_surql_default(
            &field,
            &table,
            &FieldType::Duration,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &registry,
        );
        assert_eq!(default.as_deref(), Some("0ns"));
    }

    fn base_define_config(default: Option<&str>) -> DefineConfig {
        DefineConfig {
            select_permissions: None,
            update_permissions: None,
            create_permissions: None,
            data_type: None,
            should_skip: false,
            default: default.map(|s| s.to_string()),
            default_always: None,
            value: None,
            assert: None,
            readonly: None,
            flexible: None,
            computed: None,
            comment: None,
        }
    }

    fn variant(name: &str, is_default: bool) -> Variant {
        Variant {
            name: name.to_string(),
            wire: Default::default(),
            data: None,
            doccom: None,
            annotations: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
            is_default,
        }
    }

    fn inline_struct_variant(name: &str, is_default: bool, payload: StructConfig) -> Variant {
        Variant {
            wire: Default::default(),
            data: Some(VariantData::InlineStruct(payload)),
            ..variant(name, is_default)
        }
    }

    /// Payload with one explicit `#[define_field_statement(default(10))]` field
    /// and one field relying on the type-derived fallback.
    fn threshold_payload(variant_name: &str) -> StructConfig {
        StructConfig {
            struct_name: variant_name.to_string(),
            fields: vec![
                StructField {
                    field_name: "threshold".to_string(),
                    field_type: FieldType::U32,
                    define_config: Some(base_define_config(Some("10"))),
                    ..Default::default()
                },
                StructField {
                    field_name: "tags".to_string(),
                    field_type: FieldType::Vec(Box::new(FieldType::String)),
                    define_config: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    fn tagged_union(name: &str, variants: Vec<Variant>) -> TaggedUnion {
        TaggedUnion {
            resolve_only: false,
            enum_name: name.to_string(),
            variants,
            representation: EnumRepresentation::Untagged,
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: crate::types::Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
        }
    }

    #[test]
    fn enum_default_attribute_is_honored() {
        let enum_name = "CardOrRow".to_string();
        let card_or_row = tagged_union(
            &enum_name,
            vec![
                variant("Card", false),
                variant("Table", true),
                variant("List", false),
            ],
        );
        let mut enums = BTreeMap::new();
        enums.insert(enum_name.clone(), card_or_row);
        let app_structs = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"some_field".to_string(),
            &"some_table".to_string(),
            &FieldType::Other(enum_name),
            &enums,
            &app_structs,
            &registry,
        );

        assert_eq!(result.as_deref(), Some("'Table'"));
    }

    #[test]
    fn enum_without_default_attribute_falls_back_to_first_variant() {
        let enum_name = "Color".to_string();
        let color = tagged_union(
            &enum_name,
            vec![
                variant("Red", false),
                variant("Green", false),
                variant("Blue", false),
            ],
        );
        let mut enums = BTreeMap::new();
        enums.insert(enum_name.clone(), color);
        let app_structs = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"some_field".to_string(),
            &"some_table".to_string(),
            &FieldType::Other(enum_name),
            &enums,
            &app_structs,
            &registry,
        );

        assert_eq!(result.as_deref(), Some("'Red'"));
    }

    #[test]
    fn nested_struct_field_define_default_is_honored_in_parent_literal() {
        let enum_name = "CardOrRow".to_string();
        let card_or_row = tagged_union(
            &enum_name,
            vec![variant("Card", false), variant("Table", true)],
        );
        let mut enums = BTreeMap::new();
        enums.insert(enum_name.clone(), card_or_row);

        let overview_settings = StructConfig {
            struct_name: "OverviewSettings".to_string(),
            fields: vec![
                StructField {
                    field_name: "row_height".to_string(),
                    field_type: FieldType::String,
                    define_config: Some(base_define_config(Some("\"Medium\""))),
                    ..Default::default()
                },
                StructField {
                    field_name: "card_or_row".to_string(),
                    field_type: FieldType::Other(enum_name.clone()),
                    // Intentionally no explicit define_config default; rely on
                    // the enum's #[default] marker.
                    define_config: None,
                    ..Default::default()
                },
                StructField {
                    field_name: "per_page".to_string(),
                    field_type: FieldType::U32,
                    define_config: Some(base_define_config(Some("10"))),
                    ..Default::default()
                },
                StructField {
                    field_name: "column_configs".to_string(),
                    field_type: FieldType::Vec(Box::new(FieldType::String)),
                    define_config: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let mut app_structs = BTreeMap::new();
        app_structs.insert("OverviewSettings".to_string(), overview_settings);
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"lorecast_section_overview_settings".to_string(),
            &"user".to_string(),
            &FieldType::Other("OverviewSettings".to_string()),
            &enums,
            &app_structs,
            &registry,
        );

        assert_eq!(
            result.as_deref(),
            Some(
                "{ row_height: \"Medium\", card_or_row: 'Table', per_page: 10, column_configs: [] }"
            )
        );
    }

    #[test]
    fn inline_variant_payload_defaults_are_used_in_default_literal() {
        // An inline (anonymous) variant payload is not registered in
        // `app_structs`, so it must be built from its own fields (each
        // subfield's define_config default first, type-derived fallback
        // otherwise), exactly like an embedded struct.
        let enum_name = "Strategy".to_string();
        let strategy = tagged_union(
            &enum_name,
            vec![
                variant("Fixed", false),
                inline_struct_variant("Custom", true, threshold_payload("Custom")),
            ],
        );
        let mut enums = BTreeMap::new();
        enums.insert(enum_name.clone(), strategy);
        let app_structs = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"strategy".to_string(),
            &"job".to_string(),
            &FieldType::Other(enum_name),
            &enums,
            &app_structs,
            &registry,
        );

        // Untagged representation: the default is the payload object itself.
        assert_eq!(result.as_deref(), Some("{ threshold: 10, tags: [] }"));
    }

    #[test]
    fn inline_variant_payload_defaults_merge_with_internal_tag() {
        let enum_name = "Strategy".to_string();
        let strategy = TaggedUnion {
            representation: EnumRepresentation::InternallyTagged {
                tag: "kind".to_string(),
            },
            ..tagged_union(
                &enum_name,
                vec![inline_struct_variant(
                    "Custom",
                    true,
                    threshold_payload("Custom"),
                )],
            )
        };
        let mut enums = BTreeMap::new();
        enums.insert(enum_name.clone(), strategy);
        let app_structs = BTreeMap::new();
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"strategy".to_string(),
            &"job".to_string(),
            &FieldType::Other(enum_name),
            &enums,
            &app_structs,
            &registry,
        );

        assert_eq!(
            result.as_deref(),
            Some("{ kind: 'Custom', threshold: 10, tags: [] }")
        );
    }

    #[test]
    fn required_link_has_no_default_but_optional_link_defaults_to_null() {
        let link = FieldType::RecordLink(Box::new(FieldType::Other("Customer".to_string())));
        let default_of = |field_type: &FieldType| {
            field_type_to_surql_default(
                &"customer".to_string(),
                &"order".to_string(),
                field_type,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &ForeignTypeRegistry::default(),
            )
        };

        assert_eq!(default_of(&link), None);
        assert_eq!(
            default_of(&FieldType::Struct(vec![(
                "owner".to_string(),
                link.clone()
            )])),
            None
        );
        assert_eq!(
            default_of(&FieldType::Option(Box::new(link))).as_deref(),
            Some("NULL")
        );
    }
}
