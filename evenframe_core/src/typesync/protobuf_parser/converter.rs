//! Converts Protocol Buffers descriptors to evenframe internal types.

use crate::types::{FieldType, StructConfig, StructField, TaggedUnion, Variant};
use prost_types::{
    field_descriptor_proto::{Label, Type},
    DescriptorProto, EnumDescriptorProto, FieldDescriptorProto, FileDescriptorSet,
};
use std::collections::HashMap;

/// Result of parsing and converting a Protocol Buffers schema.
#[derive(Debug, Clone)]
pub struct ProtobufParseResult {
    /// Converted struct configurations (from messages).
    pub structs: HashMap<String, StructConfig>,
    /// Converted enum configurations.
    pub enums: HashMap<String, TaggedUnion>,
    /// The package name from the schema, if any.
    pub package: Option<String>,
    /// Warnings generated during conversion.
    pub warnings: Vec<String>,
}

impl ProtobufParseResult {
    pub fn new() -> Self {
        Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            package: None,
            warnings: Vec::new(),
        }
    }
}

impl Default for ProtobufParseResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a FileDescriptorSet to evenframe types.
pub fn convert_descriptor_set(fds: &FileDescriptorSet) -> ProtobufParseResult {
    let mut result = ProtobufParseResult::new();

    for file in &fds.file {
        // Take package from first file with a package
        if result.package.is_none() && file.package.is_some() {
            result.package = file.package.clone();
        }

        let prefix = file.package.as_deref().unwrap_or("");

        // Convert messages
        for message in &file.message_type {
            convert_message(message, prefix, &mut result);
        }

        // Convert enums
        for enum_type in &file.enum_type {
            let tagged_union = convert_enum(enum_type);
            result.enums.insert(enum_type.name().to_string(), tagged_union);
        }
    }

    result
}

fn convert_message(
    message: &DescriptorProto,
    prefix: &str,
    result: &mut ProtobufParseResult,
) {
    let name = message.name().to_string();
    let full_name = if prefix.is_empty() {
        name.clone()
    } else {
        format!("{}.{}", prefix, name)
    };

    // Convert fields
    let fields = message
        .field
        .iter()
        .map(convert_field)
        .collect();

    let struct_config = StructConfig {
        struct_name: name.clone(),
        fields,
        validators: Vec::new(),
    };

    result.structs.insert(name.clone(), struct_config);

    // Handle nested types
    let nested_prefix = if prefix.is_empty() {
        name.clone()
    } else {
        full_name.clone()
    };

    for nested in &message.nested_type {
        // Skip map entry types (synthetic messages for map fields)
        if nested.options.as_ref().map(|o| o.map_entry()).unwrap_or(false) {
            continue;
        }
        convert_message(nested, &nested_prefix, result);
    }

    // Handle nested enums
    for enum_type in &message.enum_type {
        let tagged_union = convert_enum(enum_type);
        result.enums.insert(enum_type.name().to_string(), tagged_union);
    }
}

fn convert_field(field: &FieldDescriptorProto) -> StructField {
    let field_name = field.name().to_string();
    let field_type = convert_field_type(field);

    // Handle repeated fields
    let final_type = if field.label() == Label::Repeated {
        // Check if this is a map field (has type_name ending with "Entry")
        if let Some(type_name) = field.type_name.as_ref() {
            if type_name.ends_with("Entry") {
                // Map field - convert to HashMap with string keys
                FieldType::HashMap(Box::new(FieldType::String), Box::new(field_type))
            } else {
                FieldType::Vec(Box::new(field_type))
            }
        } else {
            FieldType::Vec(Box::new(field_type))
        }
    } else if field.label() == Label::Optional && !is_proto3_scalar(field) {
        // Optional wrapper for proto2 optional fields
        FieldType::Option(Box::new(field_type))
    } else {
        field_type
    };

    StructField {
        field_name,
        field_type: final_type,
        edge_config: None,
        define_config: None,
        format: None,
        validators: Vec::new(),
        always_regenerate: false,
    }
}

fn convert_field_type(field: &FieldDescriptorProto) -> FieldType {
    match field.r#type() {
        Type::Double => FieldType::F64,
        Type::Float => FieldType::F32,
        Type::Int64 | Type::Sfixed64 | Type::Sint64 => FieldType::I64,
        Type::Uint64 | Type::Fixed64 => FieldType::U64,
        Type::Int32 | Type::Sfixed32 | Type::Sint32 => FieldType::I32,
        Type::Fixed32 | Type::Uint32 => FieldType::U32,
        Type::Bool => FieldType::Bool,
        Type::String => FieldType::String,
        Type::Bytes => FieldType::Vec(Box::new(FieldType::U8)),
        Type::Enum | Type::Message => {
            // Extract type name without leading dot
            let type_name = field.type_name.as_ref()
                .map(|s| s.trim_start_matches('.').to_string())
                .unwrap_or_else(|| "Unknown".to_string());

            // Use just the last segment of the type name
            let short_name = type_name.rsplit('.').next().unwrap_or(&type_name).to_string();
            FieldType::Other(short_name)
        }
        Type::Group => {
            // Groups are deprecated in proto2, treat as message
            FieldType::Other("Group".to_string())
        }
    }
}

fn is_proto3_scalar(field: &FieldDescriptorProto) -> bool {
    // In proto3, scalar types are not wrapped in Option by default
    matches!(
        field.r#type(),
        Type::Double
            | Type::Float
            | Type::Int64
            | Type::Uint64
            | Type::Int32
            | Type::Uint32
            | Type::Fixed64
            | Type::Fixed32
            | Type::Bool
            | Type::String
            | Type::Bytes
            | Type::Sfixed32
            | Type::Sfixed64
            | Type::Sint32
            | Type::Sint64
    )
}

fn convert_enum(enum_type: &EnumDescriptorProto) -> TaggedUnion {
    let variants = enum_type
        .value
        .iter()
        .map(|v| Variant {
            name: v.name().to_string(),
            data: None, // Protobuf enums don't have associated data
        })
        .collect();

    TaggedUnion {
        enum_name: enum_type.name().to_string(),
        variants,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_simple_message() -> DescriptorProto {
        DescriptorProto {
            name: Some("Person".to_string()),
            field: vec![
                FieldDescriptorProto {
                    name: Some("name".to_string()),
                    number: Some(1),
                    label: Some(Label::Optional as i32),
                    r#type: Some(Type::String as i32),
                    ..Default::default()
                },
                FieldDescriptorProto {
                    name: Some("age".to_string()),
                    number: Some(2),
                    label: Some(Label::Optional as i32),
                    r#type: Some(Type::Int32 as i32),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_convert_simple_message() {
        let mut result = ProtobufParseResult::new();
        convert_message(&create_simple_message(), "", &mut result);

        assert!(result.structs.contains_key("Person"));
        let person = &result.structs["Person"];
        assert_eq!(person.fields.len(), 2);
        assert_eq!(person.fields[0].field_name, "name");
        assert_eq!(person.fields[1].field_name, "age");
    }

    #[test]
    fn test_convert_enum() {
        let enum_type = EnumDescriptorProto {
            name: Some("Status".to_string()),
            value: vec![
                prost_types::EnumValueDescriptorProto {
                    name: Some("UNKNOWN".to_string()),
                    number: Some(0),
                    ..Default::default()
                },
                prost_types::EnumValueDescriptorProto {
                    name: Some("ACTIVE".to_string()),
                    number: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let tagged_union = convert_enum(&enum_type);
        assert_eq!(tagged_union.enum_name, "Status");
        assert_eq!(tagged_union.variants.len(), 2);
        assert_eq!(tagged_union.variants[0].name, "UNKNOWN");
        assert_eq!(tagged_union.variants[1].name, "ACTIVE");
    }

    #[test]
    fn test_convert_repeated_field() {
        let message = DescriptorProto {
            name: Some("Container".to_string()),
            field: vec![FieldDescriptorProto {
                name: Some("items".to_string()),
                number: Some(1),
                label: Some(Label::Repeated as i32),
                r#type: Some(Type::String as i32),
                ..Default::default()
            }],
            ..Default::default()
        };

        let mut result = ProtobufParseResult::new();
        convert_message(&message, "", &mut result);

        let container = &result.structs["Container"];
        assert!(matches!(
            container.fields[0].field_type,
            FieldType::Vec(ref inner) if **inner == FieldType::String
        ));
    }
}
