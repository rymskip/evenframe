use crate::types::FieldType;
use serde_json::Value;

/// Convert a JSON value (already extracted from our struct) into the SurrealDB
/// syntax, guided by a FieldType.  Strings get single quotes in SurrealDB,
/// numeric/bool remain unquoted, arrays get bracketed, etc. This function
/// includes the special logic for EvenframeRecordId (no quotes).
pub fn to_surreal_string(field_type: &FieldType, value: &Value) -> String {
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
        FieldType::Other(_) => value.to_string(),
        FieldType::Decimal => {
            if value.is_string() {
                value.as_str().unwrap_or("0.0").to_string()
            } else if value.is_number() {
                value.to_string()
            } else {
                "0.0".to_string()
            }
        }
        FieldType::F32
        | FieldType::F64
        | FieldType::OrderedFloat(_)
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
        FieldType::EvenframeRecordId => {
            let id_string = value.as_str().unwrap_or_default();
            id_string.to_string()
        }
        FieldType::DateTime => {
            if let Some(s) = value.as_str() {
                format!("d'{}'", escape_single_quotes(s))
            } else {
                format!("d'{}'", chrono::Utc::now().to_rfc3339())
            }
        }
        FieldType::EvenframeDuration => {
            if let Some(nanos) = value.as_i64() {
                format!("duration::from::nanos({})", nanos)
            } else if let Some(nanos) = value.as_u64() {
                format!("duration::from::nanos({})", nanos)
            } else {
                "duration::from::nanos(0)".to_string()
            }
        }
        FieldType::Timezone => {
            if let Some(s) = value.as_str() {
                format!("'{}'", escape_single_quotes(s))
            } else {
                "'UTC'".to_string()
            }
        }
        FieldType::Vec(inner_type) => {
            if let Some(array) = value.as_array() {
                let items: Vec<String> = array
                    .iter()
                    .map(|item_value| to_surreal_string(inner_type, item_value))
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
                to_surreal_string(inner_type, value)
            }
        }
        FieldType::Tuple(field_types) => {
            if let Some(arr) = value.as_array() {
                let mut parts = Vec::new();
                for (sub_ftype, sub_val) in field_types.iter().zip(arr.iter()) {
                    let s = to_surreal_string(sub_ftype, sub_val);
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
                        let s = to_surreal_string(sub_field_type, sub_val);
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
                    let val_str = to_surreal_string(value_type, v);
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
                    let val_str = to_surreal_string(value_type, v);
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
                link_string.to_string()
            } else if let Some(obj) = value.as_object() {
                if let Some(id_value) = obj.get("Id") {
                    if let Some(id_str) = id_value.as_str() {
                        format!("r{id_str}")
                    } else {
                        "null".to_string()
                    }
                } else if let Some(obj_value) = obj.get("Object") {
                    to_surreal_string(inner_ftype, obj_value)
                } else {
                    to_surreal_string(inner_ftype, value)
                }
            } else {
                "null".to_string()
            }
        }
    }
}

fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "\\'")
}
