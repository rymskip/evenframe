//! FlatBuffers Abstract Syntax Tree definitions.

use std::collections::HashMap;

/// A complete FlatBuffers schema file.
#[derive(Debug, Clone)]
pub struct FlatBuffersSchema {
    pub namespace: Option<String>,
    pub includes: Vec<String>,
    pub file_identifier: Option<String>,
    pub file_extension: Option<String>,
    pub root_type: Option<String>,
    pub tables: Vec<TableDef>,
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub unions: Vec<UnionDef>,
    pub attributes: Vec<String>,
}

impl Default for FlatBuffersSchema {
    fn default() -> Self {
        Self::new()
    }
}

impl FlatBuffersSchema {
    pub fn new() -> Self {
        Self {
            namespace: None,
            includes: Vec::new(),
            file_identifier: None,
            file_extension: None,
            root_type: None,
            tables: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            unions: Vec::new(),
            attributes: Vec::new(),
        }
    }
}

/// A FlatBuffers table definition (can have optional fields).
#[derive(Debug, Clone)]
pub struct TableDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub metadata: Metadata,
    pub doc_comment: Option<String>,
}

/// A FlatBuffers struct definition (all fields required, fixed memory layout).
#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
    pub metadata: Metadata,
    pub doc_comment: Option<String>,
}

/// A field within a table or struct.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub field_type: FbsType,
    pub default_value: Option<String>,
    pub metadata: Metadata,
    pub doc_comment: Option<String>,
}

/// A FlatBuffers enum definition.
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub underlying_type: ScalarType,
    pub values: Vec<EnumValue>,
    pub metadata: Metadata,
    pub doc_comment: Option<String>,
}

/// A single enum value.
#[derive(Debug, Clone)]
pub struct EnumValue {
    pub name: String,
    pub value: Option<i64>,
    pub doc_comment: Option<String>,
}

/// A FlatBuffers union definition.
#[derive(Debug, Clone)]
pub struct UnionDef {
    pub name: String,
    pub variants: Vec<UnionVariant>,
    pub metadata: Metadata,
    pub doc_comment: Option<String>,
}

/// A single union variant.
#[derive(Debug, Clone)]
pub struct UnionVariant {
    pub name: String,
    pub type_name: Option<String>,
}

/// FlatBuffers type representation.
#[derive(Debug, Clone, PartialEq)]
pub enum FbsType {
    Scalar(ScalarType),
    String,
    Vector(Box<FbsType>),
    Array(Box<FbsType>, usize),
    Named(String),
}

/// FlatBuffers scalar types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    Bool,
    Byte,
    UByte,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    Float,
    Double,
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Float32,
    Float64,
}

impl ScalarType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bool" => Some(Self::Bool),
            "byte" | "int8" => Some(Self::Int8),
            "ubyte" | "uint8" => Some(Self::UInt8),
            "short" | "int16" => Some(Self::Int16),
            "ushort" | "uint16" => Some(Self::UInt16),
            "int" | "int32" => Some(Self::Int32),
            "uint" | "uint32" => Some(Self::UInt32),
            "long" | "int64" => Some(Self::Int64),
            "ulong" | "uint64" => Some(Self::UInt64),
            "float" | "float32" => Some(Self::Float32),
            "double" | "float64" => Some(Self::Float64),
            _ => None,
        }
    }
}

/// Metadata (attributes) attached to types or fields.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub attributes: HashMap<String, Option<String>>,
}

impl Metadata {
    pub fn new() -> Self {
        Self {
            attributes: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).and_then(|v| v.as_deref())
    }

    pub fn has(&self, key: &str) -> bool {
        self.attributes.contains_key(key)
    }
}
