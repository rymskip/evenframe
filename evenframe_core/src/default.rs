use super::schemasync::*;
use crate::types::{FieldType, StructConfig, StructField, TaggedUnion, VariantData};
use convert_case::{Case, Casing};
use rand::{rng, seq::IndexedRandom};
use std::collections::HashMap;
use tracing::{debug, trace};

pub fn field_type_to_default_value(
    field_type: &FieldType,
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
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
        FieldType::DateTime => {
            trace!("Generating default for DateTime type");
            r#""2024-01-01T00:00:00Z""#.to_string()
        }
        FieldType::EvenframeDuration => {
            trace!("Generating default for EvenframeDuration type");
            "0".to_string() // nanoseconds
        }
        FieldType::Timezone => {
            trace!("Generating default for Timezone type");
            r#""UTC""#.to_string() // IANA timezone string
        }
        FieldType::Unit => {
            trace!("Generating default for Unit type");
            "undefined".to_string()
        }
        FieldType::Decimal => {
            trace!("Generating default for Decimal type");
            r#""0""#.to_string()
        }
        FieldType::OrderedFloat(inner) => {
            trace!(
                "Generating default for OrderedFloat with inner: {:?}",
                inner
            );
            field_type_to_default_value(inner, structs, enums)
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
        FieldType::EvenframeRecordId => {
            trace!("Generating default for EvenframeRecordId");
            "''".to_string()
        }
        FieldType::Tuple(inner_types) => {
            trace!(
                "Generating default for Tuple with {} types",
                inner_types.len()
            );
            let tuple_defaults: Vec<String> = inner_types
                .iter()
                .map(|ty| field_type_to_default_value(ty, structs, enums))
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
                        field_type_to_default_value(ftype, structs, enums)
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
                if let Some(chosen_variant) = enum_schema.variants.choose(&mut rng) {
                    trace!("Chosen variant: {}", chosen_variant.name);
                    // If the variant has data, generate a default for it.
                    if let Some(variant_data) = &chosen_variant.data {
                        let variant_data_field_type = match variant_data {
                            VariantData::InlineStruct(enum_struct) => {
                                &FieldType::Other(enum_struct.struct_name.clone())
                            }
                            VariantData::DataStructureRef(field_type) => field_type,
                        };
                        let data_default =
                            field_type_to_default_value(variant_data_field_type, structs, enums);
                        return data_default;
                    } else {
                        // A variant without data
                        return format!("'\"{}\"", chosen_variant.name);
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
                            field_type_to_default_value(&table_field.field_type, structs, enums)
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

/// Generate default values for SurrealDB queries (CREATE/UPDATE statements)
pub fn field_type_to_surql_default(
    field_name: &String,
    table_name: &String,
    field_type: &FieldType,
    enums: &HashMap<String, TaggedUnion>,
    app_structs: &HashMap<String, StructConfig>,
    persistable_structs: &HashMap<String, TableConfig>,
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
        FieldType::DateTime => {
            // Generate current timestamp in SurrealDB datetime format
            trace!("Generating SURQL default for DateTime");
            "d'2024-01-01T00:00:00Z'".to_string()
        }
        FieldType::EvenframeDuration => {
            // Default duration of 0 nanoseconds
            trace!("Generating SURQL default for EvenframeDuration");
            "duration::from_nanos(0)".to_string()
        }
        FieldType::Timezone => {
            // Default timezone UTC
            trace!("Generating SURQL default for Timezone");
            "'UTC'".to_string()
        }
        FieldType::Unit => {
            trace!("Generating SURQL default for Unit");
            "NULL".to_string()
        }
        FieldType::Decimal => {
            trace!("Generating SURQL default for Decimal");
            "0.00dec".to_string()
        }
        FieldType::OrderedFloat(inner) => {
            trace!(
                "Generating SURQL default for OrderedFloat with inner: {:?}",
                inner
            );
            "0.0f".to_string() // OrderedFloat is treated as float
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
        FieldType::EvenframeRecordId => {
            // For id fields, let SurrealDB auto-generate
            debug!("Generating RecordId default for field '{}'", field_name);
            "NONE".to_string()
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
                            persistable_structs
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
            // Check if it's an enum
            if let Some(enum_schema) = enums.values().find(|e| e.enum_name == *name) {
                trace!(
                    "Found enum '{}' with {} variants",
                    name,
                    enum_schema.variants.len()
                );
                let chosen_variant = &enum_schema.variants[0];
                if let Some(variant_data) = &chosen_variant.data {
                    let variant_data_field_type = match variant_data {
                        VariantData::InlineStruct(enum_struct) => {
                            &FieldType::Other(enum_struct.struct_name.clone())
                        }
                        VariantData::DataStructureRef(field_type) => field_type,
                    };
                    // For enum with data, return the data's default
                    field_type_to_surql_default(
                        field_name,
                        table_name,
                        variant_data_field_type,
                        enums,
                        app_structs,
                        persistable_structs,
                    )
                } else {
                    // For simple enum variant
                    format!("'{}'", chosen_variant.name)
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
                        format!(
                            "{}: {}",
                            table_field.field_name.to_case(Case::Snake),
                            field_type_to_surql_default(
                                &table_field.field_name,
                                table_name,
                                &table_field.field_type,
                                enums,
                                app_structs,
                                persistable_structs
                            )
                        )
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

pub fn field_type_to_surreal_type(
    field_name: &String,
    table_name: &String,
    field_type: &FieldType,
    enums: &HashMap<String, TaggedUnion>,
    app_structs: &HashMap<String, StructConfig>,
    persistable_structs: &HashMap<String, TableConfig>,
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
        FieldType::DateTime => {
            trace!("Converting DateTime to SurrealDB type");
            ("datetime".to_string(), false, None)
        }
        FieldType::EvenframeDuration => {
            trace!("Converting EvenframeDuration to SurrealDB type");
            ("duration".to_string(), false, None)
        }
        FieldType::Timezone => {
            trace!("Converting Timezone to SurrealDB type");
            ("string".to_string(), false, None)
        }
        FieldType::Decimal => {
            trace!("Converting Decimal to SurrealDB type");
            ("decimal".to_string(), false, None)
        }
        FieldType::OrderedFloat(inner) => {
            trace!(
                "Converting OrderedFloat to SurrealDB type with inner: {:?}",
                inner
            );
            ("float".to_string(), false, None) // OrderedFloat is treated as float
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
        FieldType::EvenframeRecordId => {
            let type_str = if field_name == "id" {
                debug!("Creating record type for id field in table {}", table_name);
                format!("record<{}>", table_name)
            } else {
                debug!("Creating generic record type for field {}", field_name);
                "record<any>".to_string()
            };
            (type_str, false, None)
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
            );
            (inner_type, needs_wildcard, wildcard_type)
        }
        FieldType::Other(name) => {
            debug!(
                "Processing Other type '{}' for SurrealDB type conversion",
                name
            );
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
                            let variant_data_field_type = match variant_data {
                                VariantData::InlineStruct(enum_struct) => {
                                    &FieldType::Other(enum_struct.struct_name.clone())
                                }
                                VariantData::DataStructureRef(field_type) => field_type,
                            };
                            let (variant_type, _, _) = field_type_to_surreal_type(
                                field_name,
                                table_name,
                                variant_data_field_type,
                                enums,
                                app_structs,
                                persistable_structs,
                            );
                            variant_type
                        } else {
                            format!("\"{}\"", v.name)
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
