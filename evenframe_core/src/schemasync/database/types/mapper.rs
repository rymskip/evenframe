//! Type Mapper Trait and Utilities
//!
//! Provides the TypeMapper trait for converting Evenframe's FieldType
//! to database-specific type strings and value formatting.

use crate::types::FieldType;

/// Trait for mapping Evenframe types to database-specific types.
///
/// Each database provider implements this trait to handle type conversion
/// and value formatting for their specific database dialect.
pub trait TypeMapper: Send + Sync {
    /// Map a FieldType to the database's native type string.
    ///
    /// # Examples
    /// - String -> "TEXT" (Postgres), "VARCHAR(255)" (MySQL), "TEXT" (SQLite), "string" (SurrealDB)
    /// - I64 -> "BIGINT" (Postgres), "BIGINT" (MySQL), "INTEGER" (SQLite), "int" (SurrealDB)
    fn field_type_to_native(&self, field_type: &FieldType) -> String;

    /// Format a JSON value as a string suitable for use in a query.
    ///
    /// Handles proper escaping and type-specific formatting.
    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String;

    /// Check if the database supports native arrays.
    ///
    /// PostgreSQL supports arrays natively (INTEGER[], TEXT[], etc.)
    /// MySQL, SQLite, and SurrealDB use JSON for arrays.
    fn supports_native_arrays(&self) -> bool;

    /// Check if the database supports JSONB type.
    ///
    /// PostgreSQL has JSONB, others use JSON or TEXT.
    fn supports_jsonb(&self) -> bool;

    /// Check if the database supports native enum types.
    ///
    /// PostgreSQL can CREATE TYPE ... AS ENUM.
    /// MySQL has inline ENUM.
    /// SQLite uses CHECK constraints.
    fn supports_native_enums(&self) -> bool;

    /// Check if the database supports INTERVAL type for durations.
    ///
    /// PostgreSQL has INTERVAL.
    /// Others store as integer (nanoseconds) or TEXT.
    fn supports_interval(&self) -> bool;

    /// Get the identifier quote character for this database.
    ///
    /// PostgreSQL and SQLite use double quotes: "identifier"
    /// MySQL uses backticks: `identifier`
    /// SurrealDB uses backticks: `identifier`
    fn quote_char(&self) -> char;

    /// Quote an identifier (table name, column name, etc.)
    fn quote_identifier(&self, name: &str) -> String {
        let q = self.quote_char();
        format!("{}{}{}", q, name, q)
    }

    /// Get the string quote character for this database.
    ///
    /// Most databases use single quotes: 'string'
    fn string_quote_char(&self) -> char {
        '\''
    }

    /// Escape a string value for use in queries.
    fn escape_string(&self, value: &str) -> String {
        let q = self.string_quote_char();
        // Escape single quotes by doubling them
        let escaped = value.replace(q, &format!("{}{}", q, q));
        format!("{}{}{}", q, escaped, q)
    }

    /// Get the NULL literal for this database.
    fn null_literal(&self) -> &'static str {
        "NULL"
    }

    /// Get the boolean TRUE literal.
    fn true_literal(&self) -> &'static str {
        "TRUE"
    }

    /// Get the boolean FALSE literal.
    fn false_literal(&self) -> &'static str {
        "FALSE"
    }

    /// Format a datetime value for this database.
    ///
    /// PostgreSQL: '2024-01-01T00:00:00Z'::TIMESTAMPTZ
    /// MySQL: '2024-01-01 00:00:00'
    /// SQLite: '2024-01-01T00:00:00Z' (ISO 8601 string)
    /// SurrealDB: d'2024-01-01T00:00:00Z'
    fn format_datetime(&self, value: &str) -> String;

    /// Format a duration value for this database.
    ///
    /// PostgreSQL: INTERVAL '1 day 2 hours'
    /// MySQL/SQLite: nanoseconds as integer
    /// SurrealDB: duration::from::nanos(...)
    fn format_duration(&self, nanos: i64) -> String;

    /// Format an array value for this database.
    fn format_array(&self, field_type: &FieldType, values: &[serde_json::Value]) -> String;

    /// Get the auto-increment/serial type for primary keys.
    fn auto_increment_type(&self) -> &'static str;

    /// Get the UUID type for this database.
    fn uuid_type(&self) -> &'static str;

    /// Get the default UUID generation expression.
    fn uuid_generate_expr(&self) -> Option<&'static str>;
}

/// Default implementations for common type mapping operations
pub mod defaults {
    use super::*;

    /// Format a JSON value to a SQL-compatible string (generic implementation)
    pub fn format_json_value(value: &serde_json::Value, mapper: &dyn TypeMapper) -> String {
        match value {
            serde_json::Value::Null => mapper.null_literal().to_string(),
            serde_json::Value::Bool(b) => {
                if *b {
                    mapper.true_literal().to_string()
                } else {
                    mapper.false_literal().to_string()
                }
            }
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => mapper.escape_string(s),
            serde_json::Value::Array(arr) => {
                let formatted: Vec<String> = arr
                    .iter()
                    .map(|v| format_json_value(v, mapper))
                    .collect();
                format!("[{}]", formatted.join(", "))
            }
            serde_json::Value::Object(obj) => {
                let pairs: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| format!("{}: {}", mapper.escape_string(k), format_json_value(v, mapper)))
                    .collect();
                format!("{{{}}}", pairs.join(", "))
            }
        }
    }

    /// Convert FieldType to a generic SQL type (override in specific mappers)
    pub fn default_sql_type(field_type: &FieldType) -> String {
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
            FieldType::Decimal => "NUMERIC".to_string(),
            FieldType::DateTime => "TIMESTAMP".to_string(),
            FieldType::EvenframeDuration => "BIGINT".to_string(), // nanoseconds
            FieldType::Timezone => "TEXT".to_string(),
            FieldType::EvenframeRecordId => "TEXT".to_string(),
            FieldType::Unit => "".to_string(), // Skip unit types
            FieldType::OrderedFloat(inner) => default_sql_type(inner),
            FieldType::Option(inner) => default_sql_type(inner), // Same type, just nullable
            FieldType::Vec(_) => "JSON".to_string(),
            FieldType::Tuple(_) => "JSON".to_string(),
            FieldType::Struct(_) => "JSON".to_string(),
            FieldType::HashMap(_, _) => "JSON".to_string(),
            FieldType::BTreeMap(_, _) => "JSON".to_string(),
            FieldType::RecordLink(_) => "TEXT".to_string(), // Foreign key reference
            FieldType::Other(name) => format!("/* unknown: {} */ TEXT", name),
        }
    }

    /// Check if a FieldType is a primitive that can be used in native arrays
    pub fn is_primitive_for_array(field_type: &FieldType) -> bool {
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
                | FieldType::Decimal
                | FieldType::DateTime
        )
    }
}
