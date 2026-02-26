use crate::default::field_type_to_default_value;
use crate::types::StructConfig;
use crate::types::{FieldType, TaggedUnion, VariantData};
use crate::typesync::doc_comment::format_jsdoc;
use convert_case::{Case, Casing};
use std::collections::HashMap;
use tracing;

pub fn field_type_to_arktype(
    field_type: &FieldType,
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
) -> String {
    tracing::trace!(field_type = ?field_type, "Converting field type to Arktype");
    match field_type {
        FieldType::String => "'string'".to_string(),
        FieldType::Char => "'string'".to_string(),
        FieldType::Bool => "'boolean'".to_string(),
        FieldType::Unit => "'null'".to_string(),
        FieldType::Decimal => "'number'".to_string(),
        FieldType::OrderedFloat(_inner) => "'number'".to_string(), // OrderedFloat is treated as number
        FieldType::F32 | FieldType::F64 => "'number'".to_string(),
        FieldType::I8
        | FieldType::I16
        | FieldType::I32
        | FieldType::I64
        | FieldType::I128
        | FieldType::Isize => "'number'".to_string(),
        FieldType::U8
        | FieldType::U16
        | FieldType::U32
        | FieldType::U64
        | FieldType::U128
        | FieldType::Usize => "'number'".to_string(),
        FieldType::EvenframeRecordId => r#""string""#.to_string(),
        FieldType::DateTime => "'string'".to_string(),
        FieldType::EvenframeDuration => "'number'".to_string(), // nanoseconds
        FieldType::Timezone => "'string'".to_string(),          // IANA timezone string

        FieldType::Tuple(types) => {
            let types_str = types
                .iter()
                .map(|t| field_type_to_arktype(t, structs, enums))
                .collect::<Vec<String>>()
                .join(", ");
            format!("[{}]", types_str)
        }

        FieldType::Struct(fields) => {
            let fields_str = fields
                .iter()
                .map(|(name, field_type)| {
                    format!(
                        "{}: {}",
                        name,
                        field_type_to_arktype(field_type, structs, enums)
                    )
                })
                .collect::<Vec<String>>()
                .join(", ");
            format!("{{ {} }}", fields_str)
        }

        FieldType::Option(inner) => {
            format!(
                "[[{}, '|', 'undefined'], '|', 'null']",
                field_type_to_arktype(inner, structs, enums)
            )
        }

        FieldType::Vec(inner) => {
            format!("[{}, '[]']", field_type_to_arktype(inner, structs, enums))
        }

        FieldType::HashMap(key, value) => {
            format!(
                "'Record<{}, {}>'",
                field_type_to_arktype(key, structs, enums).replace('\'', ""),
                field_type_to_arktype(value, structs, enums).replace('\'', "")
            )
        }
        FieldType::BTreeMap(key, value) => {
            format!(
                "'Record<{}, {}>'",
                field_type_to_arktype(key, structs, enums).replace('\'', ""),
                field_type_to_arktype(value, structs, enums).replace('\'', "")
            )
        }

        FieldType::RecordLink(inner) => format!(
            r#"[{}, "|",  "string"]"#,
            field_type_to_arktype(inner, structs, enums)
        ),

        FieldType::Other(type_name) => {
            // Try to find a matching struct
            for struct_config in structs.values() {
                if struct_config.struct_name == *type_name {
                    return format!("'{}'", type_name).to_case(Case::Pascal);
                }
            }

            // Try to find a matching enum
            for schema_enum in enums.values() {
                if schema_enum.enum_name == *type_name {
                    return format!("'{}'", type_name).to_case(Case::Pascal);
                }
            }

            if let Some(enum_def) = enums.values().find(|e| e.enum_name == *type_name) {
                let variants: Vec<String> = enum_def
                    .variants
                    .iter()
                    .map(|variant| {
                        if let Some(variant_data) = &variant.data {
                            let variant_data_field_type = match variant_data {
                                VariantData::InlineStruct(enum_struct) => {
                                    &FieldType::Other(enum_struct.struct_name.clone())
                                }
                                VariantData::DataStructureRef(field_type) => field_type,
                            };
                            field_type_to_arktype(variant_data_field_type, structs, enums)
                        } else {
                            format!("'{}'", variant.name.to_case(Case::Pascal))
                        }
                    })
                    .collect();
                return variants.join(" | ");
            }

            // If no match found, return the type as is
            format!("'{}'", type_name).to_case(Case::Pascal)
        }
    }
}

pub fn generate_arktype_type_string(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    print_types: bool,
) -> String {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        print_types = print_types,
        "Generating Arktype type string"
    );
    let mut output = String::new();
    let mut scope_output = String::new();
    let mut types_output = String::new();
    let mut defaults_output = String::new();

    scope_output.push_str("export const bindings = scope({\n\n");

    // First, process all enums
    for schema_enum in enums.values() {
        // Write doc comment if present
        if let Some(ref doc) = schema_enum.doccom {
            scope_output.push_str(&format_jsdoc(doc, ""));
        }

        // Write the Arktype binding name
        scope_output.push_str(&format!(
            "{}: ",
            schema_enum.enum_name.to_case(Case::Pascal)
        ));

        // We'll accumulate the "nesting" into this string.
        let mut union_ast = String::new();

        for (i, variant) in schema_enum.variants.iter().enumerate() {
            // Convert the variant into either a data type or a literal union piece
            let item_str = if let Some(variant_data) = &variant.data {
                let variant_data_field_type = match variant_data {
                    VariantData::InlineStruct(enum_struct) => {
                        &FieldType::Other(enum_struct.struct_name.clone())
                    }
                    VariantData::DataStructureRef(field_type) => field_type,
                };
                field_type_to_arktype(variant_data_field_type, structs, enums)
            } else {
                // For simple string variants, e.g. ["===", "Residential"]
                format!("['===', '{}']", variant.name)
            };

            // If this is our first variant, it becomes the entire union so far,
            // otherwise we nest the "union so far" together with the new item.
            if i == 0 {
                union_ast = item_str;
            } else {
                union_ast = format!("[{}, '|', {}]", union_ast, item_str);
            }
        }

        // Now write out the final folded union string in your scope output
        scope_output.push_str(&format!("{},\n", union_ast));

        // And write the corresponding TypeScript type
        types_output.push_str(&format!(
            "export type {} = typeof bindings.{}.infer;\n",
            schema_enum.enum_name.to_case(Case::Pascal),
            schema_enum.enum_name.to_case(Case::Pascal)
        ));
    }

    // Then, process all structs
    tracing::debug!("Processing structs for Arktype");
    for struct_config in structs.values() {
        tracing::trace!(struct_name = %struct_config.struct_name, "Processing struct");
        let type_name = struct_config.struct_name.to_case(Case::Pascal);

        // Write doc comment if present
        if let Some(ref doc) = struct_config.doccom {
            scope_output.push_str(&format_jsdoc(doc, ""));
        }

        scope_output.push_str(&format!("{}: {{\n", type_name));
        defaults_output.push_str(&format!(
            "export const default{}: {} = {{\n",
            &type_name, &type_name
        ));

        for field in &struct_config.fields {
            let field_name = field.field_name.to_case(Case::Camel);

            // Write field doc comment if present
            if let Some(ref doc) = field.doccom {
                scope_output.push_str(&format_jsdoc(doc, "  "));
            }

            scope_output.push_str(&format!(
                "  {}: {}",
                field_name,
                field_type_to_arktype(&field.field_type, structs, enums)
            ));
            defaults_output.push_str(&format!(
                "{}: {}",
                field_name,
                field_type_to_default_value(&field.field_type, structs, enums)
            ));
            // Add a comma if it's not the last field
            if Some(field) != struct_config.fields.last() {
                scope_output.push_str(",\n");
                defaults_output.push_str(",\n");
            } else {
                scope_output.push('\n');
            }
        }

        scope_output.push_str("},\n");
        defaults_output.push_str("\n};\n");
        types_output.push_str(&format!(
            "export type {} = typeof bindings.{}.infer;\n",
            type_name, type_name
        ));
    }
    scope_output.push_str("\n});\n\n");

    if print_types {
        output.push_str(&format!(
            "{scope_output}\n{defaults_output}\n{types_output}"
        ));
    } else {
        output.push_str(&format!("{scope_output}\n{defaults_output}"));
    }

    tracing::info!(
        output_length = output.len(),
        "Arktype type string generation complete"
    );
    output
}
