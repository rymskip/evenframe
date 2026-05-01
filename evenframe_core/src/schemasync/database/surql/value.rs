use crate::registry::{get_struct_config, get_tagged_union};
use crate::types::{
    EnumRepresentation, FieldType, ForeignTypeRegistry, StructConfig, TaggedUnion, VariantData,
};
use serde_json::Value;

/// Convert a JSON value (already extracted from our struct) into the SurrealDB
/// syntax, guided by a FieldType.  Strings get single quotes in SurrealDB,
/// numeric/bool remain unquoted, arrays get bracketed, etc. This function
/// includes the special logic for EvenframeRecordId (no quotes).
pub fn to_surreal_string(
    field_type: &FieldType,
    value: &Value,
    registry: &ForeignTypeRegistry,
) -> String {
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
            } else if let Some(tagged_union) = get_tagged_union(name) {
                // Tagged union (e.g. an `EventKind` field). Without this
                // branch the call falls through to
                // `to_surreal_string_inferred`, which has no way to know
                // that nested fields like `kind.resources` are
                // `RecordLink<Resource>` and emits them as quoted
                // strings — which SurrealDB then refuses to coerce to
                // `record<resource>`. Walking the variant's struct
                // config gives every nested field its real `FieldType`.
                tagged_union_to_surreal_string(&tagged_union, value, registry)
            } else if let Some(struct_config) = get_struct_config(name) {
                // Plain (non-table) embedded struct. Same reasoning as
                // tagged unions — walk fields with their real types so
                // nested `RecordLink<T>` / typed primitives (datetime,
                // decimal, etc.) survive the round trip.
                struct_config_to_surreal_string(&struct_config, value, registry)
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

/// Walk a `TaggedUnion` value with full field-type information so nested
/// `RecordLink<T>`, datetime, decimal, etc. fields produce correct
/// SurrealQL syntax (record refs, `d'…'`, bare numbers) instead of being
/// emitted as quoted strings via `to_surreal_string_inferred`.
///
/// Honors the union's `representation`:
/// - `ExternallyTagged`: `{ "VariantName": { …fields… } }` or `"VariantName"`
/// - `InternallyTagged { tag }`: `{ tag: "VariantName", …fields… }`
/// - `AdjacentlyTagged { tag, content }`: `{ tag: "VariantName", content: { …fields… } }`
/// - `Untagged`: best-effort match against each variant's struct fields
fn tagged_union_to_surreal_string(
    tu: &TaggedUnion,
    value: &Value,
    registry: &ForeignTypeRegistry,
) -> String {
    let tu = tu.effective();

    // Pull `(variant_name, payload_obj_or_none)` out of the value
    // according to the union's serde representation.
    let (variant_name, variant_obj) = match (&tu.representation, value) {
        (EnumRepresentation::ExternallyTagged, Value::String(s)) => (s.clone(), None),
        (EnumRepresentation::ExternallyTagged, Value::Object(obj)) => {
            // Object should have exactly one key — the variant name —
            // mapped to the variant's data.
            if let Some((k, v)) = obj.iter().next() {
                (k.clone(), Some(v.clone()))
            } else {
                return to_surreal_string_inferred(value);
            }
        }
        (EnumRepresentation::InternallyTagged { tag }, Value::Object(obj)) => {
            let name = obj
                .get(tag.as_str())
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            // Strip the tag so the remaining fields are the variant data.
            let mut without_tag = obj.clone();
            without_tag.remove(tag.as_str());
            (name, Some(Value::Object(without_tag)))
        }
        (EnumRepresentation::AdjacentlyTagged { tag, content }, Value::Object(obj)) => {
            let name = obj
                .get(tag.as_str())
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            (name, obj.get(content.as_str()).cloned())
        }
        (EnumRepresentation::Untagged, _) => {
            // Without a discriminator we can't pick a variant
            // structurally here. Fall back to inference.
            return to_surreal_string_inferred(value);
        }
        _ => return to_surreal_string_inferred(value),
    };

    let Some(variant) = tu.variants.iter().find(|v| v.name == variant_name) else {
        return to_surreal_string_inferred(value);
    };

    let mut pairs: Vec<String> = Vec::new();

    // Re-emit the discriminator the same way it arrived.
    match &tu.representation {
        EnumRepresentation::InternallyTagged { tag } => {
            pairs.push(format!("{}: '{}'", tag, escape_single_quotes(&variant_name)));
        }
        EnumRepresentation::AdjacentlyTagged { tag, .. } => {
            pairs.push(format!("{}: '{}'", tag, escape_single_quotes(&variant_name)));
        }
        _ => {}
    }

    // Walk the variant's inline-struct fields (if any) with their real
    // field types. Variants with `DataStructureRef` or no data fall
    // through to the default case.
    if let Some(VariantData::InlineStruct(struct_config)) =
        variant.data.as_ref().map(|d| match d {
            VariantData::InlineStruct(sc) => VariantData::InlineStruct(sc.clone()),
            VariantData::DataStructureRef(ft) => VariantData::DataStructureRef(ft.clone()),
        })
    {
        if let Some(payload_obj) = variant_obj.as_ref().and_then(|v| v.as_object()) {
            for field in &struct_config.effective().fields {
                if let Some(sub_val) = payload_obj.get(&field.field_name) {
                    let s = to_surreal_string(&field.field_type, sub_val, registry);
                    pairs.push(format!("{}: {}", field.field_name, s));
                }
            }
        }
    }

    // Adjacently-tagged unions wrap the variant payload under `content`.
    if let EnumRepresentation::AdjacentlyTagged { content, .. } = &tu.representation {
        // Replace the body fields we just emitted with a nested
        // `content: { … }` object that matches serde's adjacent shape.
        let body_pairs: Vec<String> = pairs
            .iter()
            .filter(|p| !p.starts_with(&format!("{}:", content)) && !p.starts_with(&format!("{}: ", content)))
            .cloned()
            .collect();
        let tag_pair = body_pairs
            .iter()
            .find(|p| p.contains(": '"))
            .cloned()
            .unwrap_or_default();
        let inner_pairs: Vec<String> = body_pairs
            .into_iter()
            .filter(|p| p != &tag_pair)
            .collect();
        return format!(
            "{{ {}, {}: {{ {} }} }}",
            tag_pair,
            content,
            inner_pairs.join(", ")
        );
    }

    // Externally-tagged with payload: `{ VariantName: { …fields… } }`.
    if matches!(tu.representation, EnumRepresentation::ExternallyTagged) && variant_obj.is_some() {
        return format!(
            "{{ '{}': {{ {} }} }}",
            escape_single_quotes(&variant_name),
            pairs.join(", ")
        );
    }
    // Externally-tagged unit variant: `'VariantName'`.
    if matches!(tu.representation, EnumRepresentation::ExternallyTagged) && variant_obj.is_none() {
        return format!("'{}'", escape_single_quotes(&variant_name));
    }

    format!("{{ {} }}", pairs.join(", "))
}

/// Walk a plain (non-table) struct value with full field-type information
/// so nested `RecordLink<T>` and other typed primitives serialize to the
/// right SurrealQL form. Without this, `FieldType::Other(StructName)`
/// fell through to `to_surreal_string_inferred` and lost type info on
/// every nested field.
fn struct_config_to_surreal_string(
    sc: &StructConfig,
    value: &Value,
    registry: &ForeignTypeRegistry,
) -> String {
    let sc = sc.effective();
    let Some(obj) = value.as_object() else {
        return to_surreal_string_inferred(value);
    };
    let mut pairs: Vec<String> = Vec::new();
    for field in &sc.fields {
        if let Some(sub_val) = obj.get(&field.field_name) {
            let s = to_surreal_string(&field.field_type, sub_val, registry);
            pairs.push(format!("{}: {}", field.field_name, s));
        }
    }
    format!("{{ {} }}", pairs.join(", "))
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
