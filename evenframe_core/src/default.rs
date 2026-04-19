#[cfg(feature = "surrealdb")]
use crate::schemasync::TableConfig;
#[cfg(feature = "surrealdb")]
use crate::types::StructField;
use crate::types::{EnumRepresentation, FieldType, StructConfig, TaggedUnion, VariantData};
use convert_case::{Case, Casing};
use rand::{rng, seq::IndexedRandom};
use std::collections::HashMap;
use tracing::{debug, trace};

pub fn field_type_to_default_value(
    field_type: &FieldType,
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    trace!("Generating default value for field type: {:?}", field_type);
    let result = match field_type {
        FieldType::String | FieldType::Char => {
            trace!("Generating default for String/Char type");
            r#""""#.to_string()
        }
        FieldType::Bool => {
            trace!("Generating default for Bool type");
            "false".to_string()
        }
        FieldType::Unit => {
            trace!("Generating default for Unit type");
            "undefined".to_string()
        }
        FieldType::F32
        | FieldType::F64
        | FieldType::I8
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
        | FieldType::Usize => {
            trace!("Generating default for numeric type");
            "0".to_string()
        }
        FieldType::Tuple(inner_types) => {
            trace!(
                "Generating default for Tuple with {} types",
                inner_types.len()
            );
            let tuple_defaults: Vec<String> = inner_types
                .iter()
                .map(|ty| field_type_to_default_value(ty, structs, enums, registry))
                .collect();
            format!("[{}]", tuple_defaults.join(", "))
        }
        FieldType::Struct(fields) => {
            trace!("Generating default for Struct with {} fields", fields.len());
            let fields_str = fields
                .iter()
                .map(|(name, ftype)| {
                    format!(
                        "{}: {}",
                        name.to_case(Case::Camel),
                        field_type_to_default_value(ftype, structs, enums, registry)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {} }}", fields_str)
        }
        FieldType::Option(inner) => {
            // You can decide whether to produce `null` or `undefined` or something else.
            // For TypeScript, `null` is a more direct representation of "no value."
            trace!("Generating default for Option type with inner: {:?}", inner);
            "null".to_string()
        }
        FieldType::Vec(inner) => {
            // Returns an empty array as the default
            // but recursively if you wanted an "example entry" you could do:
            //   format!("[{}]", field_type_to_default_value(inner, structs, enums))
            trace!("Generating default for Vec type with inner: {:?}", inner);
            "[]".to_string()
        }
        FieldType::HashMap(key, value) => {
            // Return an empty object as default
            trace!(
                "Generating default for HashMap with key: {:?}, value: {:?}",
                key, value
            );
            "{}".to_string()
        }

        FieldType::BTreeMap(key, value) => {
            // Return an empty object as default
            trace!(
                "Generating default for BTreeMap with key: {:?}, value: {:?}",
                key, value
            );
            "{}".to_string()
        }

        FieldType::RecordLink(inner) => {
            // Could produce "null" or "0" depending on your usage pattern.
            // We'll pick "null" for "unlinked".
            trace!("Generating default for RecordLink with inner: {:?}", inner);
            "''".to_string()
        }
        FieldType::Other(name) => {
            // 0) Check if it's a configured foreign type
            if let Some(ftc) = registry.lookup(name) {
                return ftc.default_value_ts.clone();
            }

            // 1) If this is an enum, pick a random variant.
            // 2) Otherwise if it matches a known table, produce a default object for that table.
            // 3) If neither, fall back to 'undefined'.
            debug!("Generating default for Other type: {}", name);

            // First check for an enum of this name
            if let Some(enum_schema) = enums.values().find(|e| e.enum_name == *name) {
                debug!(
                    "Found enum {} with {} variants",
                    name,
                    enum_schema.variants.len()
                );
                let mut rng = rng();
                let chosen_variant = enum_schema
                    .variants
                    .iter()
                    .find(|v| v.is_default)
                    .or_else(|| enum_schema.variants.choose(&mut rng));
                if let Some(chosen_variant) = chosen_variant {
                    trace!("Chosen variant: {}", chosen_variant.name);
                    // If the variant has data, generate a default for it.
                    if let Some(variant_data) = &chosen_variant.data {
                        let inner_default = match variant_data {
                            VariantData::InlineStruct(enum_struct) => field_type_to_default_value(
                                &FieldType::Other(enum_struct.struct_name.clone()),
                                structs,
                                enums,
                                registry,
                            ),
                            VariantData::DataStructureRef(field_type) => {
                                field_type_to_default_value(field_type, structs, enums, registry)
                            }
                        };
                        return match &enum_schema.representation {
                            EnumRepresentation::ExternallyTagged => {
                                format!("{{ {}: {} }}", chosen_variant.name, inner_default)
                            }
                            EnumRepresentation::InternallyTagged { tag } => {
                                if let VariantData::InlineStruct(_) = variant_data {
                                    // Merge tag into the struct — strip outer braces and prepend tag
                                    let trimmed = inner_default.trim();
                                    if trimmed.starts_with('{') && trimmed.ends_with('}') {
                                        let inner = &trimmed[1..trimmed.len() - 1];
                                        format!(
                                            "{{ {}: '\"{}\"', {} }}",
                                            tag,
                                            chosen_variant.name,
                                            inner.trim()
                                        )
                                    } else {
                                        format!("{{ {}: '\"{}\"' }}", tag, chosen_variant.name)
                                    }
                                } else {
                                    // DataStructureRef — serde doesn't support this, fall back to external
                                    format!("{{ {}: {} }}", chosen_variant.name, inner_default)
                                }
                            }
                            EnumRepresentation::AdjacentlyTagged { tag, content } => {
                                format!(
                                    "{{ {}: '\"{}\"', {}: {} }}",
                                    tag, chosen_variant.name, content, inner_default
                                )
                            }
                            EnumRepresentation::Untagged => inner_default,
                        };
                    } else {
                        // A unit variant without data
                        return match &enum_schema.representation {
                            EnumRepresentation::InternallyTagged { tag }
                            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                                format!("{{ {}: '\"{}\"' }}", tag, chosen_variant.name)
                            }
                            _ => format!("'\"{}\"", chosen_variant.name),
                        };
                    }
                } else {
                    // If no variants, fallback to undefined
                    return "undefined".to_string();
                }
            }

            if let Some(struct_config) = structs.values().find(|struct_config| {
                struct_config.struct_name.to_case(Case::Pascal) == name.to_case(Case::Pascal)
            }) {
                debug!(
                    "Found struct {} with {} fields",
                    name,
                    struct_config.fields.len()
                );
                // We treat this similarly to a struct:
                let fields_str = struct_config
                    .fields
                    .iter()
                    .map(|table_field| {
                        format!(
                            "{}: {}",
                            table_field.field_name.to_case(Case::Camel),
                            field_type_to_default_value(
                                &table_field.field_type,
                                structs,
                                enums,
                                registry
                            )
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {} }}", fields_str)
            } else {
                // Not an enum or known table
                trace!(
                    "Type {} not found in enums or structs, returning undefined",
                    name
                );
                "undefined".to_string()
            }
        }
    };
    trace!("Generated default value: {}", result);
    result
}

#[cfg(feature = "surrealdb")]
/// Generate default values for SurrealDB queries (CREATE/UPDATE statements)
pub fn field_type_to_surql_default(
    field_name: &String,
    table_name: &String,
    field_type: &FieldType,
    enums: &HashMap<String, TaggedUnion>,
    app_structs: &HashMap<String, StructConfig>,
    persistable_structs: &HashMap<String, TableConfig>,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    trace!(
        "Generating SURQL default for field '{}' in table '{}', type: {:?}",
        field_name, table_name, field_type
    );
    let result = match field_type {
        FieldType::String | FieldType::Char => {
            trace!("Generating SURQL default for String/Char");
            "\'\'".to_string()
        }
        FieldType::Bool => {
            trace!("Generating SURQL default for Bool");
            "false".to_string()
        }
        FieldType::Unit => {
            trace!("Generating SURQL default for Unit");
            "NULL".to_string()
        }
        FieldType::F32 | FieldType::F64 => {
            trace!("Generating SURQL default for float type");
            "0.0f".to_string()
        }
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
        | FieldType::Usize => {
            trace!("Generating SURQL default for integer type");
            "0".to_string()
        }
        FieldType::Tuple(inner_types) => {
            trace!(
                "Generating SURQL default for Tuple with {} types",
                inner_types.len()
            );
            let tuple_defaults: Vec<String> = inner_types
                .iter()
                .map(|ty| {
                    field_type_to_surql_default(
                        field_name,
                        table_name,
                        ty,
                        enums,
                        app_structs,
                        persistable_structs,
                        registry,
                    )
                })
                .collect();
            format!("[{}]", tuple_defaults.join(", "))
        }
        FieldType::Struct(fields) => {
            trace!(
                "Generating SURQL default for Struct with {} fields",
                fields.len()
            );
            let fields_str = fields
                .iter()
                .map(|(name, ftype)| {
                    format!(
                        "{}: {}",
                        name.to_case(Case::Snake), // SurrealDB typically uses snake_case
                        field_type_to_surql_default(
                            field_name,
                            table_name,
                            ftype,
                            enums,
                            app_structs,
                            persistable_structs,
                            registry
                        )
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {} }}", fields_str)
        }
        FieldType::Option(inner) => {
            trace!(
                "Generating SURQL default for Option with inner: {:?}",
                inner
            );
            "NULL".to_string()
        }
        FieldType::Vec(inner) => {
            trace!("Generating SURQL default for Vec with inner: {:?}", inner);
            "[]".to_string()
        }
        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
            trace!(
                "Generating SURQL default for Map with key: {:?}, value: {:?}",
                key, value
            );
            "{}".to_string()
        }
        FieldType::RecordLink(inner) => {
            trace!(
                "Generating SURQL default for RecordLink with inner: {:?}",
                inner
            );
            "NULL".to_string()
        }
        FieldType::Other(name) => {
            debug!("Processing Other type '{}' for SURQL default", name);

            // Check if it's a configured foreign type
            if let Some(ftc) = registry.lookup(name) {
                return ftc.default_value_surql.clone();
            }

            // Check if it's an enum
            if let Some(enum_schema) = enums.values().find(|e| e.enum_name == *name) {
                trace!(
                    "Found enum '{}' with {} variants",
                    name,
                    enum_schema.variants.len()
                );
                let chosen_variant = enum_schema
                    .variants
                    .iter()
                    .find(|v| v.is_default)
                    .unwrap_or(&enum_schema.variants[0]);
                if let Some(variant_data) = &chosen_variant.data {
                    let inner_default = match variant_data {
                        VariantData::InlineStruct(enum_struct) => field_type_to_surql_default(
                            field_name,
                            table_name,
                            &FieldType::Other(enum_struct.struct_name.clone()),
                            enums,
                            app_structs,
                            persistable_structs,
                            registry,
                        ),
                        VariantData::DataStructureRef(field_type) => field_type_to_surql_default(
                            field_name,
                            table_name,
                            field_type,
                            enums,
                            app_structs,
                            persistable_structs,
                            registry,
                        ),
                    };
                    match &enum_schema.representation {
                        EnumRepresentation::ExternallyTagged => {
                            format!("{{ {}: {} }}", chosen_variant.name, inner_default)
                        }
                        EnumRepresentation::InternallyTagged { tag } => {
                            if let VariantData::InlineStruct(_) = variant_data {
                                let trimmed = inner_default.trim();
                                if trimmed.starts_with('{') && trimmed.ends_with('}') {
                                    let inner = &trimmed[1..trimmed.len() - 1];
                                    format!(
                                        "{{ {}: '{}', {} }}",
                                        tag,
                                        chosen_variant.name,
                                        inner.trim()
                                    )
                                } else {
                                    format!("{{ {}: '{}' }}", tag, chosen_variant.name)
                                }
                            } else {
                                format!("{{ {}: {} }}", chosen_variant.name, inner_default)
                            }
                        }
                        EnumRepresentation::AdjacentlyTagged { tag, content } => {
                            format!(
                                "{{ {}: '{}', {}: {} }}",
                                tag, chosen_variant.name, content, inner_default
                            )
                        }
                        EnumRepresentation::Untagged => inner_default,
                    }
                } else {
                    // For simple enum variant
                    match &enum_schema.representation {
                        EnumRepresentation::InternallyTagged { tag }
                        | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                            format!("{{ {}: '{}' }}", tag, chosen_variant.name)
                        }
                        _ => format!("'{}'", chosen_variant.name),
                    }
                }
            }
            // Check if it's a struct
            else if let Some(struct_config) = app_structs.values().find(|struct_config| {
                struct_config.struct_name.to_case(Case::Pascal) == name.to_case(Case::Pascal)
            }) {
                debug!(
                    "Found app struct '{}' with {} fields",
                    name,
                    struct_config.fields.len()
                );
                let fields_str = struct_config
                    .fields
                    .iter()
                    .map(|table_field| {
                        let value = table_field
                            .define_config
                            .as_ref()
                            .and_then(|dc| dc.default.as_deref())
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| {
                                field_type_to_surql_default(
                                    &table_field.field_name,
                                    table_name,
                                    &table_field.field_type,
                                    enums,
                                    app_structs,
                                    persistable_structs,
                                    registry,
                                )
                            });
                        format!("{}: {}", table_field.field_name.to_case(Case::Snake), value)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {} }}", fields_str)
            }
            // Check if it's a persistable struct (table reference)
            else if persistable_structs.get(name).is_some() {
                // For record links to other tables, default to NULL
                debug!("Found persistable struct '{}', defaulting to NULL", name);
                "NULL".to_string()
            } else {
                trace!("Type '{}' not found, defaulting to NULL", name);
                "NULL".to_string()
            }
        }
    };
    trace!("Generated SURQL default: {}", result);
    result
}

#[cfg(feature = "surrealdb")]
pub fn field_type_to_surreal_type(
    field_name: &String,
    table_name: &String,
    field_type: &FieldType,
    enums: &HashMap<String, TaggedUnion>,
    app_structs: &HashMap<String, StructConfig>,
    persistable_structs: &HashMap<String, TableConfig>,
    registry: &crate::types::ForeignTypeRegistry,
) -> (String, bool, Option<String>) {
    trace!(
        "Converting field '{}' in table '{}' to SurrealDB type, field_type: {:?}",
        field_name, table_name, field_type
    );
    let result = match field_type {
        FieldType::String | FieldType::Char => {
            trace!("Converting String/Char to SurrealDB type");
            ("string".to_string(), false, None)
        }
        FieldType::Bool => {
            trace!("Converting Bool to SurrealDB type");
            ("bool".to_string(), false, None)
        }
        FieldType::F32 | FieldType::F64 => {
            trace!("Converting float to SurrealDB type");
            ("float".to_string(), false, None)
        }
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
        | FieldType::Usize => {
            trace!("Converting integer to SurrealDB type");
            ("int".to_string(), false, None)
        }
        FieldType::Unit => {
            trace!("Converting Unit to SurrealDB type");
            ("any".to_string(), false, None)
        }
        FieldType::HashMap(_key, value) => {
            trace!("Converting HashMap to SurrealDB type");
            let (value_type, _, _) = field_type_to_surreal_type(
                field_name,
                table_name,
                value,
                enums,
                app_structs,
                persistable_structs,
                registry,
            );
            ("object".to_string(), true, Some(value_type))
        }
        FieldType::BTreeMap(_key, value) => {
            trace!("Converting BTreeMap to SurrealDB type");
            let (value_type, _, _) = field_type_to_surreal_type(
                field_name,
                table_name,
                value,
                enums,
                app_structs,
                persistable_structs,
                registry,
            );
            ("object".to_string(), true, Some(value_type))
        }
        FieldType::RecordLink(inner) => {
            trace!(
                "Converting RecordLink to SurrealDB type with inner: {:?}",
                inner
            );
            let (inner_type, needs_wildcard, wildcard_type) = field_type_to_surreal_type(
                field_name,
                table_name,
                inner,
                enums,
                app_structs,
                persistable_structs,
                registry,
            );
            (inner_type, needs_wildcard, wildcard_type)
        }
        FieldType::Other(name) => {
            debug!(
                "Processing Other type '{}' for SurrealDB type conversion",
                name
            );

            // Check if it's a configured foreign type
            if let Some(ftc) = registry.lookup(name) {
                let type_str = if field_name == "id" {
                    if let Some(ref id_fmt) = ftc.surrealdb_id_format {
                        id_fmt.replace("{table_name}", table_name)
                    } else {
                        ftc.surrealdb.clone()
                    }
                } else if let Some(ref non_id_fmt) = ftc.surrealdb_non_id_format {
                    non_id_fmt.clone()
                } else {
                    ftc.surrealdb.clone()
                };
                return (type_str, false, None);
            }

            // If this type name is defined as an enum, output its union literal.
            if let Some(enum_def) = enums.get(name) {
                debug!(
                    "Found enum '{}' with {} variants",
                    name,
                    enum_def.variants.len()
                );
                let variants: Vec<String> = enum_def
                    .variants
                    .iter()
                    .map(|v| {
                        if let Some(variant_data) = &v.data {
                            let inner_type = match variant_data {
                                VariantData::InlineStruct(enum_struct) => {
                                    let (t, _, _) = field_type_to_surreal_type(
                                        field_name,
                                        table_name,
                                        &FieldType::Other(enum_struct.struct_name.clone()),
                                        enums,
                                        app_structs,
                                        persistable_structs,
                                        registry,
                                    );
                                    t
                                }
                                VariantData::DataStructureRef(field_type) => {
                                    let (t, _, _) = field_type_to_surreal_type(
                                        field_name,
                                        table_name,
                                        field_type,
                                        enums,
                                        app_structs,
                                        persistable_structs,
                                        registry,
                                    );
                                    t
                                }
                            };
                            match &enum_def.representation {
                                EnumRepresentation::ExternallyTagged => {
                                    format!("{{ {}: {} }}", v.name, inner_type)
                                }
                                EnumRepresentation::InternallyTagged { tag } => {
                                    if let VariantData::InlineStruct(_) = variant_data {
                                        let trimmed = inner_type.trim();
                                        if trimmed.starts_with('{') && trimmed.ends_with('}') {
                                            let inner = &trimmed[1..trimmed.len() - 1];
                                            format!(
                                                "{{ {}: \"{}\", {} }}",
                                                tag,
                                                v.name,
                                                inner.trim()
                                            )
                                        } else {
                                            format!("{{ {}: \"{}\" }}", tag, v.name)
                                        }
                                    } else {
                                        format!("{{ {}: {} }}", v.name, inner_type)
                                    }
                                }
                                EnumRepresentation::AdjacentlyTagged { tag, content } => {
                                    format!(
                                        "{{ {}: \"{}\", {}: {} }}",
                                        tag, v.name, content, inner_type
                                    )
                                }
                                EnumRepresentation::Untagged => inner_type,
                            }
                        } else {
                            match &enum_def.representation {
                                EnumRepresentation::InternallyTagged { tag }
                                | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                                    format!("{{ {}: \"{}\" }}", tag, v.name)
                                }
                                _ => format!("\"{}\"", v.name),
                            }
                        }
                    })
                    .collect();
                (variants.join(" | "), false, None)
            } else if let Some(app_struct) = app_structs.get(name) {
                debug!(
                    "Found app struct '{}' with {} fields for type conversion",
                    name,
                    app_struct.fields.len()
                );
                let field_defs: Vec<String> = app_struct
                    .fields
                    .iter()
                    .map(|f: &StructField| {
                        let (field_type, _, _) = field_type_to_surreal_type(
                            &f.field_name,
                            table_name,
                            &f.field_type,
                            enums,
                            app_structs,
                            persistable_structs,
                            registry,
                        );
                        format!("{}: {}", f.field_name, field_type)
                    })
                    .collect();

                (format!("{{ {} }}", field_defs.join(", ")), false, None)
            } else if persistable_structs.get(name).is_some() {
                debug!("Creating record type for persistable struct '{}'", name);
                (
                    format!("record<{}>", name.to_case(Case::Snake)),
                    false,
                    None,
                )
            } else {
                trace!("Type '{}' not found in any category, using as-is", name);
                (name.clone(), false, None)
            }
        }
        FieldType::Option(inner) => {
            trace!(
                "Converting Option to SurrealDB type with inner: {:?}",
                inner
            );
            let (inner_type, needs_wildcard, wildcard_type) = field_type_to_surreal_type(
                field_name,
                table_name,
                inner,
                enums,
                app_structs,
                persistable_structs,
                registry,
            );
            (
                format!("null | {}", inner_type),
                needs_wildcard,
                wildcard_type,
            )
        }
        FieldType::Vec(inner) => {
            trace!("Converting Vec to SurrealDB type with inner: {:?}", inner);
            let (inner_type, _, _) = field_type_to_surreal_type(
                field_name,
                table_name,
                inner,
                enums,
                app_structs,
                persistable_structs,
                registry,
            );
            (format!("array<{}>", inner_type), false, None)
        }
        FieldType::Tuple(inner_types) => {
            trace!(
                "Converting Tuple to SurrealDB type with {} types",
                inner_types.len()
            );
            let inner: Vec<String> = inner_types
                .iter()
                .map(|t| {
                    let (inner_type, _, _) = field_type_to_surreal_type(
                        field_name,
                        table_name,
                        t,
                        enums,
                        app_structs,
                        persistable_structs,
                        registry,
                    );
                    inner_type
                })
                .collect();
            // (SurrealDB does not have a dedicated tuple type so we wrap it as an array)
            (format!("array<{}>", inner.join(", ")), false, None)
        }
        FieldType::Struct(fields) => {
            trace!(
                "Converting Struct to SurrealDB type with {} fields",
                fields.len()
            );
            let field_defs: Vec<String> = fields
                .iter()
                .map(|(name, t)| {
                    let (field_type, _, _) = field_type_to_surreal_type(
                        field_name,
                        table_name,
                        t,
                        enums,
                        app_structs,
                        persistable_structs,
                        registry,
                    );
                    format!("{}: {}", name, field_type)
                })
                .collect();
            (format!("{{ {} }}", field_defs.join(", ")), false, None)
        }
    };
    trace!("Generated SurrealDB type: {:?}", result);
    result
}

#[cfg(all(test, feature = "surrealdb"))]
mod tests {
    use super::*;
    use crate::schemasync::DefineConfig;
    use crate::types::{
        EnumRepresentation, ForeignTypeRegistry, StructConfig, StructField, TaggedUnion, Variant,
    };
    use std::collections::HashMap;

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
            data: None,
            doccom: None,
            annotations: vec![],
            output_override: None,
            raw_attributes: HashMap::new(),
            is_default,
        }
    }

    fn tagged_union(name: &str, variants: Vec<Variant>) -> TaggedUnion {
        TaggedUnion {
            enum_name: name.to_string(),
            variants,
            representation: EnumRepresentation::Untagged,
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: crate::types::Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: HashMap::new(),
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
        let mut enums = HashMap::new();
        enums.insert(enum_name.clone(), card_or_row);
        let app_structs = HashMap::new();
        let persistable_structs = HashMap::new();
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"some_field".to_string(),
            &"some_table".to_string(),
            &FieldType::Other(enum_name),
            &enums,
            &app_structs,
            &persistable_structs,
            &registry,
        );

        assert_eq!(result, "'Table'");
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
        let mut enums = HashMap::new();
        enums.insert(enum_name.clone(), color);
        let app_structs = HashMap::new();
        let persistable_structs = HashMap::new();
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"some_field".to_string(),
            &"some_table".to_string(),
            &FieldType::Other(enum_name),
            &enums,
            &app_structs,
            &persistable_structs,
            &registry,
        );

        assert_eq!(result, "'Red'");
    }

    #[test]
    fn nested_struct_field_define_default_is_honored_in_parent_literal() {
        let enum_name = "CardOrRow".to_string();
        let card_or_row = tagged_union(
            &enum_name,
            vec![variant("Card", false), variant("Table", true)],
        );
        let mut enums = HashMap::new();
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
                    // Intentionally no explicit define_config default — rely on
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
        let mut app_structs = HashMap::new();
        app_structs.insert("OverviewSettings".to_string(), overview_settings);
        let persistable_structs = HashMap::new();
        let registry = ForeignTypeRegistry::default();

        let result = field_type_to_surql_default(
            &"lorecast_section_overview_settings".to_string(),
            &"user".to_string(),
            &FieldType::Other("OverviewSettings".to_string()),
            &enums,
            &app_structs,
            &persistable_structs,
            &registry,
        );

        assert_eq!(
            result,
            "{ row_height: \"Medium\", card_or_row: 'Table', per_page: 10, column_configs: [] }"
        );
    }
}
