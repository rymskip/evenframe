use crate::types::{FieldType, ForeignTypeRegistry};
use serde_json::Value;

/// Convert a JSON value (already extracted from our struct) into the SurrealDB
/// syntax, guided by a FieldType.  Strings get single quotes in SurrealDB,
/// numeric/bool remain unquoted, arrays get bracketed, etc. This function
/// includes the special logic for EvenframeRecordId (no quotes).
pub fn to_surreal_string(field_type: &FieldType, value: &Value, registry: &ForeignTypeRegistry) -> String {
    match field_type {
        FieldType::String | FieldType::Char => {
            let s = value.as_str().unwrap_or_default();
            format!("'{}'", escape_single_quotes(s))
        }
        FieldType::Bool => {
            if value.as_bool().unwrap_or(false) {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        FieldType::Other(name) => {
            if let Some(ftc) = registry.lookup(name) {
                match ftc.surql_value_format.as_str() {
                    "datetime" => {
                        if let Some(s) = value.as_str() {
                            format!("d'{}'", escape_single_quotes(s))
                        } else {
                            format!("d'{}'", chrono::Utc::now().to_rfc3339())
                        }
                    }
                    "duration_from_nanos" => {
                        if let Some(nanos) = value.as_i64() {
                            format!("duration::from_nanos({})", nanos)
                        } else if let Some(nanos) = value.as_u64() {
                            format!("duration::from_nanos({})", nanos)
                        } else if let Some(arr) = value.as_array() {
                            let seconds = arr.first().and_then(|v| v.as_i64()).unwrap_or(0);
                            let nanos = arr.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
                            let total_nanos = seconds * 1_000_000_000 + nanos;
                            format!("duration::from_nanos({})", total_nanos)
                        } else {
                            "duration::from_nanos(0)".to_string()
                        }
                    }
                    "quoted_string" => {
                        if let Some(s) = value.as_str() {
                            format!("'{}'", escape_single_quotes(s))
                        } else {
                            "'UTC'".to_string()
                        }
                    }
                    "decimal_number" => {
                        if value.is_string() {
                            value.as_str().unwrap_or("0.0").to_string()
                        } else if value.is_number() {
                            value.to_string()
                        } else {
                            "0.0".to_string()
                        }
                    }
                    "record_id" => {
                        let id_string = value.as_str().unwrap_or_default();
                        id_string.replace('`', "")
                    }
                    _ => to_surreal_string_inferred(value),
                }
            } else {
                to_surreal_string_inferred(value)
            }
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
            if value.is_number() {
                value.to_string()
            } else {
                "0".to_string()
            }
        }
        FieldType::Unit => "null".to_string(),
        FieldType::Vec(inner_type) => {
            if let Some(array) = value.as_array() {
                let items: Vec<String> = array
                    .iter()
                    .map(|item_value| to_surreal_string(inner_type, item_value, registry))
                    .collect();
                format!("[{}]", items.join(", "))
            } else {
                "[]".to_string()
            }
        }
        FieldType::Option(inner_type) => {
            if value.is_null() {
                "null".to_string()
            } else {
                to_surreal_string(inner_type, value, registry)
            }
        }
        FieldType::Tuple(field_types) => {
            if let Some(arr) = value.as_array() {
                let mut parts = Vec::new();
                for (sub_ftype, sub_val) in field_types.iter().zip(arr.iter()) {
                    let s = to_surreal_string(sub_ftype, sub_val, registry);
                    parts.push(s);
                }
                format!("[{}]", parts.join(", "))
            } else {
                "".to_string()
            }
        }
        FieldType::Struct(fields) => {
            if let Some(obj) = value.as_object() {
                let mut pairs = Vec::new();
                for (sub_field_name, sub_field_type) in fields {
                    if let Some(sub_val) = obj.get(sub_field_name) {
                        let s = to_surreal_string(sub_field_type, sub_val, registry);
                        pairs.push(format!("{}: {}", sub_field_name, s));
                    }
                }
                format!("{{ {} }}", pairs.join(", "))
            } else {
                "{}".to_string()
            }
        }
        FieldType::HashMap(key_type, value_type) => {
            if let Some(obj) = value.as_object() {
                let mut pairs = Vec::new();
                for (k, v) in obj {
                    let key_str = match &**key_type {
                        FieldType::String | FieldType::Char | FieldType::Other(_) => {
                            format!("'{}'", escape_single_quotes(k))
                        }
                        _ => k.clone(),
                    };
                    let val_str = to_surreal_string(value_type, v, registry);
                    pairs.push(format!("{}: {}", key_str, val_str));
                }
                format!("{{ {} }}", pairs.join(", "))
            } else {
                "{}".to_string()
            }
        }
        FieldType::BTreeMap(key_type, value_type) => {
            if let Some(obj) = value.as_object() {
                let mut pairs = Vec::new();
                for (k, v) in obj {
                    let key_str = match &**key_type {
                        FieldType::String | FieldType::Char | FieldType::Other(_) => {
                            format!("'{}'", escape_single_quotes(k))
                        }
                        _ => k.clone(),
                    };
                    let val_str = to_surreal_string(value_type, v, registry);
                    pairs.push(format!("{}: {}", key_str, val_str));
                }
                format!("{{ {} }}", pairs.join(", "))
            } else {
                "{}".to_string()
            }
        }
        FieldType::RecordLink(inner_ftype) => {
            if value.is_string() {
                let link_string = value
                    .as_str()
                    .expect("Record link value should not be None");
                link_string.replace('`', "")
            } else if let Some(obj) = value.as_object() {
                if let Some(id_value) = obj.get("Id") {
                    if let Some(id_str) = id_value.as_str() {
                        format!("r{id_str}")
                    } else {
                        "null".to_string()
                    }
                } else if let Some(id_value) = obj.get("id") {
                    // When the record was FETCHed, the full object is present
                    // with a lowercase "id" field. Extract just the ID.
                    if let Some(id_str) = id_value.as_str() {
                        id_str.replace('`', "")
                    } else {
                        "null".to_string()
                    }
                } else if let Some(obj_value) = obj.get("Object") {
                    to_surreal_string(inner_ftype, obj_value, registry)
                } else {
                    to_surreal_string(inner_ftype, value, registry)
                }
            } else {
                "null".to_string()
            }
        }
    }
}

/// Recursively convert a JSON value to SurrealQL syntax by inferring types.
/// Used for `FieldType::Other` (nested Evenframe structs) where field type
/// information is not available. Detects ISO 8601 datetimes and wraps them
/// in SurrealQL `d'...'` syntax so they are stored as proper datetime values
/// rather than strings.
fn to_surreal_string_inferred(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if is_iso8601_datetime(s) {
                format!("d'{}'", escape_single_quotes(s))
            } else {
                format!("'{}'", escape_single_quotes(s))
            }
        }
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(to_surreal_string_inferred).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(obj) => {
            let pairs: Vec<String> = obj
                .iter()
                .map(|(k, v)| format!("{}: {}", k, to_surreal_string_inferred(v)))
                .collect();
            format!("{{ {} }}", pairs.join(", "))
        }
    }
}

/// Check if a string is an ISO 8601 datetime (e.g. "2025-05-29T23:00:00Z").
fn is_iso8601_datetime(s: &str) -> bool {
    if s.len() < 20 {
        return false;
    }
    let b = s.as_bytes();
    // YYYY-MM-DDTHH:MM:SS...
    b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' && b[16] == b':'
}

fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "\\'")
}
