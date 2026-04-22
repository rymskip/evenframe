use crate::default::field_type_to_default_value;
use crate::types::StructConfig;
use crate::types::{EnumRepresentation, FieldType, TaggedUnion, Variant, VariantData};
use crate::typesync::doc_comment::format_jsdoc;
use convert_case::{Case, Casing};
use std::collections::BTreeMap;
use tracing;

/// Converts a single enum variant into its ArkType representation,
/// respecting the serde enum representation strategy.
fn variant_to_arktype(
    variant: &Variant,
    representation: &EnumRepresentation,
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    match representation {
        EnumRepresentation::ExternallyTagged => {
            if let Some(variant_data) = &variant.data {
                let inner = match variant_data {
                    VariantData::InlineStruct(enum_struct) => field_type_to_arktype(
                        &FieldType::Other(enum_struct.struct_name.clone()),
                        structs,
                        enums,
                        registry,
                    ),
                    VariantData::DataStructureRef(field_type) => {
                        field_type_to_arktype(field_type, structs, enums, registry)
                    }
                };
                format!("{{ {}: {} }}", variant.name, inner)
            } else {
                format!("['===', '{}']", variant.name)
            }
        }
        EnumRepresentation::InternallyTagged { tag } => {
            if let Some(variant_data) = &variant.data {
                match variant_data {
                    VariantData::InlineStruct(enum_struct) => {
                        // Merge the tag field into the struct fields
                        let mut fields_parts: Vec<String> =
                            vec![format!("{}: ['===', '{}']", tag, variant.name)];
                        for field in &enum_struct.fields {
                            let field_name = field.field_name.to_case(Case::Camel);
                            fields_parts.push(format!(
                                "{}: {}",
                                field_name,
                                field_type_to_arktype(&field.field_type, structs, enums, registry)
                            ));
                        }
                        format!("{{ {} }}", fields_parts.join(", "))
                    }
                    VariantData::DataStructureRef(_field_type) => {
                        // Internally tagged doesn't work well with non-struct data;
                        // fall back to externally tagged wrapping.
                        let inner = field_type_to_arktype(_field_type, structs, enums, registry);
                        format!("{{ {}: {} }}", variant.name, inner)
                    }
                }
            } else {
                // Unit variant: just the tag field
                format!("{{ {}: ['===', '{}'] }}", tag, variant.name)
            }
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            if let Some(variant_data) = &variant.data {
                let inner = match variant_data {
                    VariantData::InlineStruct(enum_struct) => field_type_to_arktype(
                        &FieldType::Other(enum_struct.struct_name.clone()),
                        structs,
                        enums,
                        registry,
                    ),
                    VariantData::DataStructureRef(field_type) => {
                        field_type_to_arktype(field_type, structs, enums, registry)
                    }
                };
                format!(
                    "{{ {}: ['===', '{}'], {}: {} }}",
                    tag, variant.name, content, inner
                )
            } else {
                // Unit variant: just the tag, no content field
                format!("{{ {}: ['===', '{}'] }}", tag, variant.name)
            }
        }
        EnumRepresentation::Untagged => {
            if let Some(variant_data) = &variant.data {
                match variant_data {
                    VariantData::InlineStruct(enum_struct) => field_type_to_arktype(
                        &FieldType::Other(enum_struct.struct_name.clone()),
                        structs,
                        enums,
                        registry,
                    ),
                    VariantData::DataStructureRef(field_type) => {
                        field_type_to_arktype(field_type, structs, enums, registry)
                    }
                }
            } else {
                format!("['===', '{}']", variant.name)
            }
        }
    }
}

pub fn field_type_to_arktype(
    field_type: &FieldType,
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> String {
    tracing::trace!(field_type = ?field_type, "Converting field type to Arktype");
    match field_type {
        FieldType::String => "'string'".to_string(),
        FieldType::Char => "'string'".to_string(),
        FieldType::Bool => "'boolean'".to_string(),
        FieldType::Unit => "'null'".to_string(),
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

        FieldType::Tuple(types) => {
            let types_str = types
                .iter()
                .map(|t| field_type_to_arktype(t, structs, enums, registry))
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
                        field_type_to_arktype(field_type, structs, enums, registry)
                    )
                })
                .collect::<Vec<String>>()
                .join(", ");
            format!("{{ {} }}", fields_str)
        }

        FieldType::Option(inner) => {
            format!(
                "[[{}, '|', 'undefined'], '|', 'null']",
                field_type_to_arktype(inner, structs, enums, registry)
            )
        }

        FieldType::Vec(inner) => {
            format!(
                "[{}, '[]']",
                field_type_to_arktype(inner, structs, enums, registry)
            )
        }

        FieldType::HashMap(key, value) => {
            format!(
                "'Record<{}, {}>'",
                field_type_to_arktype(key, structs, enums, registry).replace('\'', ""),
                field_type_to_arktype(value, structs, enums, registry).replace('\'', "")
            )
        }
        FieldType::BTreeMap(key, value) => {
            format!(
                "'Record<{}, {}>'",
                field_type_to_arktype(key, structs, enums, registry).replace('\'', ""),
                field_type_to_arktype(value, structs, enums, registry).replace('\'', "")
            )
        }

        FieldType::RecordLink(inner) => format!(
            r#"[{}, "|",  "string"]"#,
            field_type_to_arktype(inner, structs, enums, registry)
        ),

        FieldType::Other(type_name) => {
            // Check foreign type registry first
            if let Some(ftc) = registry.lookup(type_name)
                && !ftc.arktype.is_empty()
            {
                return ftc.arktype.clone();
            }

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
                        variant_to_arktype(
                            variant,
                            &enum_def.representation,
                            structs,
                            enums,
                            registry,
                        )
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
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    print_types: bool,
    registry: &crate::types::ForeignTypeRegistry,
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

    // First, process all enums (skip alias entries with output_override —
    // literal override semantics treat them as if they were never scanned in)
    for schema_enum in enums.values() {
        if schema_enum.output_override.is_some() {
            continue;
        }
        let schema_enum = schema_enum.effective();
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
            // Convert the variant into either a data type or a literal union piece,
            // respecting the serde enum representation.
            let item_str = variant_to_arktype(
                variant,
                &schema_enum.representation,
                structs,
                enums,
                registry,
            );

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

    // Then, process all structs (skip alias entries with output_override)
    tracing::debug!("Processing structs for Arktype");
    for struct_config in structs.values() {
        if struct_config.output_override.is_some() {
            continue;
        }
        let struct_config = struct_config.effective();
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
                field_type_to_arktype(&field.field_type, structs, enums, registry)
            ));
            defaults_output.push_str(&format!(
                "{}: {}",
                field_name,
                field_type_to_default_value(&field.field_type, structs, enums, registry)
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
