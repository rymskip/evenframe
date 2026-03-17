//! SQL Type Mappers
//!
//! Implementations of the TypeMapper trait for SQL databases.

use crate::schemasync::database::TypeMapper;
use crate::types::{FieldType, ForeignTypeRegistry};

/// PostgreSQL type mapper
pub struct PostgresTypeMapper<'a> {
    registry: &'a ForeignTypeRegistry,
}

impl<'a> PostgresTypeMapper<'a> {
    pub fn new(registry: &'a ForeignTypeRegistry) -> Self {
        Self { registry }
    }
}

impl<'a> TypeMapper for PostgresTypeMapper<'a> {
    fn field_type_to_native(&self, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String => "TEXT".to_string(),
            FieldType::Char => "CHAR(1)".to_string(),
            FieldType::Bool => "BOOLEAN".to_string(),
            FieldType::I8 => "SMALLINT".to_string(),
            FieldType::I16 => "SMALLINT".to_string(),
            FieldType::I32 => "INTEGER".to_string(),
            FieldType::I64 => "BIGINT".to_string(),
            FieldType::I128 => "NUMERIC(39,0)".to_string(),
            FieldType::Isize => "BIGINT".to_string(),
            FieldType::U8 => "SMALLINT".to_string(),
            FieldType::U16 => "INTEGER".to_string(),
            FieldType::U32 => "BIGINT".to_string(),
            FieldType::U64 => "NUMERIC(20,0)".to_string(),
            FieldType::U128 => "NUMERIC(39,0)".to_string(),
            FieldType::Usize => "BIGINT".to_string(),
            FieldType::F32 => "REAL".to_string(),
            FieldType::F64 => "DOUBLE PRECISION".to_string(),
            FieldType::Unit => "".to_string(), // Skip
            FieldType::Option(inner) => self.field_type_to_native(inner),
            FieldType::Vec(inner) => {
                // Use native array for primitives, JSONB for complex types
                if is_primitive(inner) {
                    format!("{}[]", self.field_type_to_native(inner))
                } else {
                    "JSONB".to_string()
                }
            }
            FieldType::Tuple(_) => "JSONB".to_string(),
            FieldType::Struct(_) => "JSONB".to_string(),
            FieldType::HashMap(_, _) => "JSONB".to_string(),
            FieldType::BTreeMap(_, _) => "JSONB".to_string(),
            FieldType::RecordLink(_) => "UUID".to_string(), // Foreign key
            FieldType::Other(name) => {
                if let Some(ftc) = self.registry.lookup(name) {
                    ftc.postgres.clone()
                } else {
                    format!("/* {} */ TEXT", name)
                }
            }
        }
    }

    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String {
        match field_type {
            FieldType::String | FieldType::Char => {
                let s = value.as_str().unwrap_or_default();
                format!("'{}'", s.replace('\'', "''"))
            }
            FieldType::Bool => {
                if value.as_bool().unwrap_or(false) { "TRUE" } else { "FALSE" }.to_string()
            }
            FieldType::Vec(_) | FieldType::Tuple(_) | FieldType::Struct(_)
            | FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) => {
                // Use JSON format
                format!("'{}'::JSONB", value.to_string().replace('\'', "''"))
            }
            FieldType::Option(inner) => {
                if value.is_null() {
                    "NULL".to_string()
                } else {
                    self.format_value(inner, value)
                }
            }
            _ => {
                if value.is_null() {
                    "NULL".to_string()
                } else if value.is_number() {
                    value.to_string()
                } else if let Some(s) = value.as_str() {
                    format!("'{}'", s.replace('\'', "''"))
                } else {
                    value.to_string()
                }
            }
        }
    }

    fn supports_native_arrays(&self) -> bool { true }
    fn supports_jsonb(&self) -> bool { true }
    fn supports_native_enums(&self) -> bool { true }
    fn supports_interval(&self) -> bool { true }
    fn quote_char(&self) -> char { '"' }

    fn format_datetime(&self, value: &str) -> String {
        format!("'{}'::TIMESTAMPTZ", value)
    }

    fn format_duration(&self, nanos: i64) -> String {
        let secs = nanos / 1_000_000_000;
        format!("INTERVAL '{} seconds'", secs)
    }

    fn format_array(&self, field_type: &FieldType, values: &[serde_json::Value]) -> String {
        let inner = if let FieldType::Vec(inner) = field_type {
            inner.as_ref()
        } else {
            &FieldType::String
        };

        if is_primitive(inner) {
            let formatted: Vec<String> = values
                .iter()
                .map(|v| self.format_value(inner, v))
                .collect();
            format!("ARRAY[{}]", formatted.join(", "))
        } else {
            format!("'{}'::JSONB", serde_json::to_string(values).unwrap_or_default().replace('\'', "''"))
        }
    }

    fn auto_increment_type(&self) -> &'static str { "SERIAL" }
    fn uuid_type(&self) -> &'static str { "UUID" }
    fn uuid_generate_expr(&self) -> Option<&'static str> { Some("gen_random_uuid()") }
}

/// MySQL type mapper
pub struct MysqlTypeMapper<'a> {
    registry: &'a ForeignTypeRegistry,
}

impl<'a> MysqlTypeMapper<'a> {
    pub fn new(registry: &'a ForeignTypeRegistry) -> Self {
        Self { registry }
    }
}

impl<'a> TypeMapper for MysqlTypeMapper<'a> {
    fn field_type_to_native(&self, field_type: &FieldType) -> String {
        match field_type {
            FieldType::String => "TEXT".to_string(),
            FieldType::Char => "CHAR(1)".to_string(),
            FieldType::Bool => "TINYINT(1)".to_string(),
            FieldType::I8 => "TINYINT".to_string(),
            FieldType::I16 => "SMALLINT".to_string(),
            FieldType::I32 => "INT".to_string(),
            FieldType::I64 => "BIGINT".to_string(),
            FieldType::I128 => "DECIMAL(39,0)".to_string(),
            FieldType::Isize => "BIGINT".to_string(),
            FieldType::U8 => "TINYINT UNSIGNED".to_string(),
            FieldType::U16 => "SMALLINT UNSIGNED".to_string(),
            FieldType::U32 => "INT UNSIGNED".to_string(),
            FieldType::U64 => "BIGINT UNSIGNED".to_string(),
            FieldType::U128 => "DECIMAL(39,0)".to_string(),
            FieldType::Usize => "BIGINT UNSIGNED".to_string(),
            FieldType::F32 => "FLOAT".to_string(),
            FieldType::F64 => "DOUBLE".to_string(),
            FieldType::Unit => "".to_string(),
            FieldType::Option(inner) => self.field_type_to_native(inner),
            FieldType::Vec(_) => "JSON".to_string(),
            FieldType::Tuple(_) => "JSON".to_string(),
            FieldType::Struct(_) => "JSON".to_string(),
            FieldType::HashMap(_, _) => "JSON".to_string(),
            FieldType::BTreeMap(_, _) => "JSON".to_string(),
            FieldType::RecordLink(_) => "VARCHAR(255)".to_string(),
            FieldType::Other(name) => {
                if let Some(ftc) = self.registry.lookup(name) {
                    ftc.mysql.clone()
                } else {
                    format!("/* {} */ TEXT", name)
                }
            }
        }
    }

    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String {
        match field_type {
            FieldType::String | FieldType::Char => {
                let s = value.as_str().unwrap_or_default();
                format!("'{}'", s.replace('\'', "''"))
            }
            FieldType::Bool => {
                if value.as_bool().unwrap_or(false) { "1" } else { "0" }.to_string()
            }
            FieldType::Vec(_) | FieldType::Tuple(_) | FieldType::Struct(_)
            | FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) => {
                format!("'{}'", value.to_string().replace('\'', "''"))
            }
            FieldType::Option(inner) => {
                if value.is_null() {
                    "NULL".to_string()
                } else {
                    self.format_value(inner, value)
                }
            }
            _ => {
                if value.is_null() {
                    "NULL".to_string()
                } else if value.is_number() {
                    value.to_string()
                } else if let Some(s) = value.as_str() {
                    format!("'{}'", s.replace('\'', "''"))
                } else {
                    value.to_string()
                }
            }
        }
    }

    fn supports_native_arrays(&self) -> bool { false }
    fn supports_jsonb(&self) -> bool { false } // MySQL has JSON but not JSONB
    fn supports_native_enums(&self) -> bool { true } // MySQL has ENUM type
    fn supports_interval(&self) -> bool { false }
    fn quote_char(&self) -> char { '`' }

    fn format_datetime(&self, value: &str) -> String {
        format!("'{}'", value)
    }

    fn format_duration(&self, nanos: i64) -> String {
        nanos.to_string()
    }

    fn format_array(&self, _field_type: &FieldType, values: &[serde_json::Value]) -> String {
        format!("'{}'", serde_json::to_string(values).unwrap_or_default().replace('\'', "''"))
    }

    fn auto_increment_type(&self) -> &'static str { "INT AUTO_INCREMENT" }
    fn uuid_type(&self) -> &'static str { "VARCHAR(36)" }
    fn uuid_generate_expr(&self) -> Option<&'static str> { Some("UUID()") }
}

/// SQLite type mapper
pub struct SqliteTypeMapper<'a> {
    registry: &'a ForeignTypeRegistry,
}

impl<'a> SqliteTypeMapper<'a> {
    pub fn new(registry: &'a ForeignTypeRegistry) -> Self {
        Self { registry }
    }
}

impl<'a> TypeMapper for SqliteTypeMapper<'a> {
    fn field_type_to_native(&self, field_type: &FieldType) -> String {
        // SQLite has dynamic typing with type affinities
        match field_type {
            FieldType::String | FieldType::Char => "TEXT".to_string(),
            FieldType::Bool => "INTEGER".to_string(),
            FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64
            | FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64
            | FieldType::Isize | FieldType::Usize => "INTEGER".to_string(),
            FieldType::I128 | FieldType::U128 => "TEXT".to_string(), // Too large for INTEGER
            FieldType::F32 | FieldType::F64 => "REAL".to_string(),
            FieldType::Unit => "".to_string(),
            FieldType::Option(inner) => self.field_type_to_native(inner),
            FieldType::Vec(_) => "TEXT".to_string(), // JSON string
            FieldType::Tuple(_) => "TEXT".to_string(),
            FieldType::Struct(_) => "TEXT".to_string(),
            FieldType::HashMap(_, _) => "TEXT".to_string(),
            FieldType::BTreeMap(_, _) => "TEXT".to_string(),
            FieldType::RecordLink(_) => "TEXT".to_string(),
            FieldType::Other(name) => {
                if let Some(ftc) = self.registry.lookup(name) {
                    ftc.sqlite.clone()
                } else {
                    "TEXT".to_string()
                }
            }
        }
    }

    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String {
        match field_type {
            FieldType::String | FieldType::Char => {
                let s = value.as_str().unwrap_or_default();
                format!("'{}'", s.replace('\'', "''"))
            }
            FieldType::Bool => {
                if value.as_bool().unwrap_or(false) { "1" } else { "0" }.to_string()
            }
            FieldType::Vec(_) | FieldType::Tuple(_) | FieldType::Struct(_)
            | FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) => {
                format!("'{}'", value.to_string().replace('\'', "''"))
            }
            FieldType::Option(inner) => {
                if value.is_null() {
                    "NULL".to_string()
                } else {
                    self.format_value(inner, value)
                }
            }
            _ => {
                if value.is_null() {
                    "NULL".to_string()
                } else if value.is_number() {
                    value.to_string()
                } else if let Some(s) = value.as_str() {
                    format!("'{}'", s.replace('\'', "''"))
                } else {
                    value.to_string()
                }
            }
        }
    }

    fn supports_native_arrays(&self) -> bool { false }
    fn supports_jsonb(&self) -> bool { false }
    fn supports_native_enums(&self) -> bool { false }
    fn supports_interval(&self) -> bool { false }
    fn quote_char(&self) -> char { '"' }

    fn format_datetime(&self, value: &str) -> String {
        format!("'{}'", value)
    }

    fn format_duration(&self, nanos: i64) -> String {
        nanos.to_string()
    }

    fn format_array(&self, _field_type: &FieldType, values: &[serde_json::Value]) -> String {
        format!("'{}'", serde_json::to_string(values).unwrap_or_default().replace('\'', "''"))
    }

    fn auto_increment_type(&self) -> &'static str { "INTEGER PRIMARY KEY" }
    fn uuid_type(&self) -> &'static str { "TEXT" }
    fn uuid_generate_expr(&self) -> Option<&'static str> { None }
}

/// Check if a FieldType is a primitive that can be used in native arrays
fn is_primitive(field_type: &FieldType) -> bool {
    matches!(
        field_type,
        FieldType::String
            | FieldType::Char
            | FieldType::Bool
            | FieldType::I8
            | FieldType::I16
            | FieldType::I32
            | FieldType::I64
            | FieldType::U8
            | FieldType::U16
            | FieldType::U32
            | FieldType::U64
            | FieldType::F32
            | FieldType::F64
    )
}
