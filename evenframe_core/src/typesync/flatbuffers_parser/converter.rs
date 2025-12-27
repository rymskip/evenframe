//! Converts FlatBuffers AST to evenframe internal types.

use super::ast::{
    EnumDef, FbsType, FieldDef, FlatBuffersSchema, ScalarType, StructDef, TableDef, UnionDef,
};
use crate::types::{FieldType, StructConfig, StructField, TaggedUnion, Variant, VariantData};
use std::collections::HashMap;

/// Result of parsing and converting a FlatBuffers schema.
#[derive(Debug, Clone)]
pub struct FlatBuffersParseResult {
    /// Converted struct configurations (from tables and structs).
    pub structs: HashMap<String, StructConfig>,
    /// Converted enum configurations.
    pub enums: HashMap<String, TaggedUnion>,
    /// The namespace from the schema, if any.
    pub namespace: Option<String>,
    /// Warnings generated during conversion.
    pub warnings: Vec<String>,
}

impl FlatBuffersParseResult {
    pub fn new() -> Self {
        Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            namespace: None,
            warnings: Vec::new(),
        }
    }
}

impl Default for FlatBuffersParseResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a FlatBuffers schema to evenframe types.
pub fn convert_schema(schema: &FlatBuffersSchema) -> FlatBuffersParseResult {
    let mut result = FlatBuffersParseResult::new();
    result.namespace = schema.namespace.clone();

    // Convert tables
    for table in &schema.tables {
        let struct_config = convert_table(table);
        result.structs.insert(table.name.clone(), struct_config);
    }

    // Convert structs (FlatBuffers structs are like tables but with fixed layout)
    for struct_def in &schema.structs {
        let struct_config = convert_struct(struct_def);
        result.structs.insert(struct_def.name.clone(), struct_config);
    }

    // Convert enums
    for enum_def in &schema.enums {
        let tagged_union = convert_enum(enum_def);
        result.enums.insert(enum_def.name.clone(), tagged_union);
    }

    // Convert unions to tagged enums
    for union_def in &schema.unions {
        let tagged_union = convert_union(union_def);
        result.enums.insert(union_def.name.clone(), tagged_union);
    }

    result
}

fn convert_table(table: &TableDef) -> StructConfig {
    let fields = table.fields.iter().map(convert_field).collect();

    StructConfig {
        struct_name: table.name.clone(),
        fields,
        validators: Vec::new(), // Validators will be extracted separately
    }
}

fn convert_struct(struct_def: &StructDef) -> StructConfig {
    let fields = struct_def.fields.iter().map(convert_field).collect();

    StructConfig {
        struct_name: struct_def.name.clone(),
        fields,
        validators: Vec::new(),
    }
}

fn convert_field(field: &FieldDef) -> StructField {
    StructField {
        field_name: field.name.clone(),
        field_type: convert_type(&field.field_type),
        edge_config: None,
        define_config: None,
        format: None,
        validators: Vec::new(), // Validators extracted separately
        always_regenerate: false,
    }
}

fn convert_type(fbs_type: &FbsType) -> FieldType {
    match fbs_type {
        FbsType::Scalar(scalar) => convert_scalar(*scalar),
        FbsType::String => FieldType::String,
        FbsType::Vector(inner) => FieldType::Vec(Box::new(convert_type(inner))),
        FbsType::Array(inner, _size) => {
            // Fixed-size arrays are treated as Vec for now
            FieldType::Vec(Box::new(convert_type(inner)))
        }
        FbsType::Named(name) => FieldType::Other(name.clone()),
    }
}

fn convert_scalar(scalar: ScalarType) -> FieldType {
    match scalar {
        ScalarType::Bool => FieldType::Bool,
        ScalarType::Byte | ScalarType::Int8 => FieldType::I8,
        ScalarType::UByte | ScalarType::UInt8 => FieldType::U8,
        ScalarType::Short | ScalarType::Int16 => FieldType::I16,
        ScalarType::UShort | ScalarType::UInt16 => FieldType::U16,
        ScalarType::Int | ScalarType::Int32 => FieldType::I32,
        ScalarType::UInt | ScalarType::UInt32 => FieldType::U32,
        ScalarType::Long | ScalarType::Int64 => FieldType::I64,
        ScalarType::ULong | ScalarType::UInt64 => FieldType::U64,
        ScalarType::Float | ScalarType::Float32 => FieldType::F32,
        ScalarType::Double | ScalarType::Float64 => FieldType::F64,
    }
}

fn convert_enum(enum_def: &EnumDef) -> TaggedUnion {
    let variants = enum_def
        .values
        .iter()
        .map(|v| Variant {
            name: v.name.clone(),
            data: None, // FlatBuffers enums don't have associated data
        })
        .collect();

    TaggedUnion {
        enum_name: enum_def.name.clone(),
        variants,
    }
}

fn convert_union(union_def: &UnionDef) -> TaggedUnion {
    let variants = union_def
        .variants
        .iter()
        .map(|v| {
            // Union variants reference other types
            let type_name = v.type_name.as_ref().unwrap_or(&v.name);
            Variant {
                name: v.name.clone(),
                data: Some(VariantData::DataStructureRef(FieldType::Other(
                    type_name.clone(),
                ))),
            }
        })
        .collect();

    TaggedUnion {
        enum_name: union_def.name.clone(),
        variants,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typesync::flatbuffers_parser::parser::parse;

    #[test]
    fn test_convert_simple_table() {
        let source = r#"
            namespace Example;

            table Person {
                name: string;
                age: int32;
                active: bool;
            }
        "#;

        let schema = parse(source).unwrap();
        let result = convert_schema(&schema);

        assert_eq!(result.namespace, Some("Example".to_string()));
        assert!(result.structs.contains_key("Person"));

        let person = &result.structs["Person"];
        assert_eq!(person.struct_name, "Person");
        assert_eq!(person.fields.len(), 3);

        assert_eq!(person.fields[0].field_name, "name");
        assert_eq!(person.fields[0].field_type, FieldType::String);

        assert_eq!(person.fields[1].field_name, "age");
        assert_eq!(person.fields[1].field_type, FieldType::I32);

        assert_eq!(person.fields[2].field_name, "active");
        assert_eq!(person.fields[2].field_type, FieldType::Bool);
    }

    #[test]
    fn test_convert_enum() {
        let source = r#"
            enum Color : byte {
                Red = 0,
                Green = 1,
                Blue = 2
            }
        "#;

        let schema = parse(source).unwrap();
        let result = convert_schema(&schema);

        assert!(result.enums.contains_key("Color"));
        let color = &result.enums["Color"];
        assert_eq!(color.enum_name, "Color");
        assert_eq!(color.variants.len(), 3);
        assert_eq!(color.variants[0].name, "Red");
        assert!(color.variants[0].data.is_none());
    }

    #[test]
    fn test_convert_union() {
        let source = r#"
            table Dog { name: string; }
            table Cat { name: string; }

            union Pet {
                Dog,
                Cat
            }
        "#;

        let schema = parse(source).unwrap();
        let result = convert_schema(&schema);

        assert!(result.enums.contains_key("Pet"));
        let pet = &result.enums["Pet"];
        assert_eq!(pet.variants.len(), 2);
        assert!(pet.variants[0].data.is_some());
    }

    #[test]
    fn test_convert_vector_types() {
        let source = r#"
            table Container {
                items: [string];
                numbers: [int32];
            }
        "#;

        let schema = parse(source).unwrap();
        let result = convert_schema(&schema);

        let container = &result.structs["Container"];
        assert!(matches!(
            container.fields[0].field_type,
            FieldType::Vec(ref inner) if **inner == FieldType::String
        ));
        assert!(matches!(
            container.fields[1].field_type,
            FieldType::Vec(ref inner) if **inner == FieldType::I32
        ));
    }

    #[test]
    fn test_convert_nested_types() {
        let source = r#"
            table Address {
                street: string;
                city: string;
            }

            table Person {
                name: string;
                address: Address;
            }
        "#;

        let schema = parse(source).unwrap();
        let result = convert_schema(&schema);

        let person = &result.structs["Person"];
        assert_eq!(person.fields[1].field_name, "address");
        assert!(matches!(
            person.fields[1].field_type,
            FieldType::Other(ref name) if name == "Address"
        ));
    }
}
