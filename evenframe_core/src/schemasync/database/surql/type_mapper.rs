//! SurrealDB Type Mapper Implementation
//!
//! Maps Evenframe's FieldType to SurrealDB native types.

use crate::schemasync::database::types::mapper::TypeMapper;
use crate::types::FieldType;

use super::value::to_surreal_string;

/// Type mapper for SurrealDB
pub struct SurrealdbTypeMapper;

impl SurrealdbTypeMapper {
    /// Map a FieldType to SurrealQL type syntax
    pub fn field_type_to_surql(&self, field_type: &FieldType) -> String {
        Self::field_type_to_surql_inner(field_type)
    }

    fn field_type_to_surql_inner(field_type: &FieldType) -> String {
        match field_type {
            FieldType::String => "string".to_string(),
            FieldType::Char => "string".to_string(),
            FieldType::Bool => "bool".to_string(),
            FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 | FieldType::I128 => {
                "int".to_string()
            }
            FieldType::Isize => "int".to_string(),
            FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 | FieldType::U128 => {
                "int".to_string()
            }
            FieldType::Usize => "int".to_string(),
            FieldType::F32 | FieldType::F64 => "float".to_string(),
            FieldType::OrderedFloat(inner) => Self::field_type_to_surql_inner(inner),
            FieldType::Decimal => "decimal".to_string(),
            FieldType::DateTime => "datetime".to_string(),
            FieldType::EvenframeDuration => "duration".to_string(),
            FieldType::Timezone => "string".to_string(),
            FieldType::EvenframeRecordId => "record".to_string(),
            FieldType::Unit => "null".to_string(),
            FieldType::Option(inner) => {
                format!("option<{}>", Self::field_type_to_surql_inner(inner))
            }
            FieldType::Vec(inner) => {
                format!("array<{}>", Self::field_type_to_surql_inner(inner))
            }
            FieldType::Tuple(_types) => {
                // SurrealDB doesn't have tuple types, use array<any>
                "array<any>".to_string()
            }
            FieldType::Struct(_) => "object".to_string(),
            FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) => "object".to_string(),
            FieldType::RecordLink(inner) => {
                // Try to extract the table name from the inner type
                if let FieldType::Other(table_name) = inner.as_ref() {
                    format!("record<{}>", table_name)
                } else {
                    "record".to_string()
                }
            }
            FieldType::Other(name) => name.clone(),
        }
    }
}

impl TypeMapper for SurrealdbTypeMapper {
    fn field_type_to_native(&self, field_type: &FieldType) -> String {
        self.field_type_to_surql(field_type)
    }

    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String {
        to_surreal_string(field_type, value)
    }

    fn supports_native_arrays(&self) -> bool {
        true // SurrealDB has native array<T> type
    }

    fn supports_jsonb(&self) -> bool {
        false // SurrealDB uses 'object' type, not JSONB
    }

    fn supports_native_enums(&self) -> bool {
        false // SurrealDB doesn't have CREATE TYPE ENUM
    }

    fn supports_interval(&self) -> bool {
        true // SurrealDB has native duration type
    }

    fn quote_char(&self) -> char {
        '`' // SurrealDB uses backticks for identifiers
    }

    fn format_datetime(&self, value: &str) -> String {
        format!("d'{}'", value)
    }

    fn format_duration(&self, nanos: i64) -> String {
        format!("duration::from_nanos({})", nanos)
    }

    fn format_array(&self, field_type: &FieldType, values: &[serde_json::Value]) -> String {
        let inner_type = if let FieldType::Vec(inner) = field_type {
            inner.as_ref()
        } else {
            &FieldType::String
        };

        let formatted: Vec<String> = values
            .iter()
            .map(|v| self.format_value(inner_type, v))
            .collect();

        format!("[{}]", formatted.join(", "))
    }

    fn auto_increment_type(&self) -> &'static str {
        "record" // SurrealDB auto-generates record IDs
    }

    fn uuid_type(&self) -> &'static str {
        "string" // UUIDs are stored as strings in SurrealDB
    }

    fn uuid_generate_expr(&self) -> Option<&'static str> {
        Some("rand::uuid::v4()") // SurrealDB function for UUID generation
    }
}
