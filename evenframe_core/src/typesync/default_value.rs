//! The TypeScript default value of a field type, which the ArkType output
//! writes for a field's `.default(...)`.

use crate::config::RECORD_ID;
use crate::error::{EvenframeError, Result};
use crate::types::{EnumRepresentation, FieldType, TaggedUnion, VariantData};
use crate::typesync::type_index::TypeIndex;
use convert_case::{Case, Casing};
use tracing::{debug, trace};

pub fn field_type_to_default_value(
    field_type: &FieldType,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    trace!("Generating default value for field type: {:?}", field_type);
    let result = match field_type {
        FieldType::String | FieldType::Char => {
            trace!("Generating default for String/Char type");
            r#""""#.to_string()
        }
        FieldType::Bool => {
            trace!("Generating default for Bool type");
            "false".to_string()
        }
        FieldType::Unit => {
            trace!("Generating default for Unit type");
            // serde writes `()` as `null`.
            "null".to_string()
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
            trace!("Generating default for numeric type");
            "0".to_string()
        }
        FieldType::Duration => {
            field_type_to_default_value(&FieldType::serde_duration(), index, registry)?
        }
        FieldType::Tuple(inner_types) => {
            trace!(
                "Generating default for Tuple with {} types",
                inner_types.len()
            );
            let tuple_defaults = inner_types
                .iter()
                .map(|ty| field_type_to_default_value(ty, index, registry))
                .collect::<Result<Vec<String>>>()?;
            format!("[{}]", tuple_defaults.join(", "))
        }
        FieldType::Struct(fields) => {
            trace!("Generating default for Struct with {} fields", fields.len());
            let fields_str = fields
                .iter()
                .map(|(name, ftype)| {
                    Ok(format!(
                        "{}: {}",
                        name.to_case(Case::Camel),
                        field_type_to_default_value(ftype, index, registry)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            format!("{{ {} }}", fields_str)
        }
        FieldType::Option(inner) => {
            // You can decide whether to produce `null` or `undefined` or something else.
            // For TypeScript, `null` is a more direct representation of "no value."
            trace!("Generating default for Option type with inner: {:?}", inner);
            "null".to_string()
        }
        FieldType::Vec(inner) => {
            trace!("Generating default for Vec type with inner: {:?}", inner);
            "[]".to_string()
        }
        FieldType::HashMap(key, value) => {
            // Return an empty object as default
            trace!(
                "Generating default for HashMap with key: {:?}, value: {:?}",
                key, value
            );
            "{}".to_string()
        }

        FieldType::BTreeMap(key, value) => {
            // Return an empty object as default
            trace!(
                "Generating default for BTreeMap with key: {:?}, value: {:?}",
                key, value
            );
            "{}".to_string()
        }

        FieldType::RecordLink(_) => match registry.lookup(RECORD_ID) {
            Some(record_id) if !record_id.default_value_ts.trim().is_empty() => {
                record_id.default_value_ts.clone()
            }
            _ => {
                return Err(EvenframeError::config(format!(
                    "a record link defaults to a record id, which needs \
                     `foreign_types.{RECORD_ID}` with a `default_value_ts`"
                )));
            }
        },
        FieldType::Other(name) => {
            // 0) Check if it's a configured foreign type
            if let Some(ftc) = registry.lookup(name) {
                if ftc.default_value_ts.trim().is_empty() {
                    return Err(EvenframeError::config(format!(
                        "foreign type '{name}' has no `default_value_ts`, which the arktype output needs for its default values"
                    )));
                }
                return Ok(ftc.default_value_ts.clone());
            }

            // An enum defaults to its `#[default]` variant, else its first; a
            // struct to an object of its fields' defaults.
            debug!("Generating default for Other type: {}", name);

            if let Some(enum_schema) = index.enum_named(name) {
                return enum_default(enum_schema, index, registry);
            }

            if let Some(struct_config) = index.struct_named(name) {
                debug!(
                    "Found struct {} with {} fields",
                    name,
                    struct_config.fields.len()
                );
                struct_fields_default(&struct_config.fields, index, registry)?
            } else {
                return Err(EvenframeError::type_sync(format!(
                    "`{name}` is neither a scanned type nor a foreign type, so it has no default \
                     value"
                )));
            }
        }
    };
    trace!("Generated default value: {}", result);
    Ok(result)
}

/// The TypeScript default for an enum: its `#[default]` variant, else its
/// first, in the enum's serde representation.
fn enum_default(
    enum_schema: &TaggedUnion,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let variant = enum_schema
        .variants
        .iter()
        .find(|variant| variant.is_default)
        .or_else(|| enum_schema.variants.first())
        .ok_or_else(|| {
            EvenframeError::config(format!(
                "enum '{}' has no variants, so it has no default value",
                enum_schema.enum_name
            ))
        })?;
    let name = serde_json::to_string(&variant.name).map_err(|error| {
        EvenframeError::config(format!("cannot encode variant {:?}: {error}", variant.name))
    })?;
    let representation = &enum_schema.representation;
    let Some(data) = &variant.data else {
        return Ok(match representation {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => format!("{{ {tag}: {name} }}"),
            EnumRepresentation::ExternallyTagged | EnumRepresentation::Untagged => name,
        });
    };
    let payload = match data {
        VariantData::InlineStruct(inline) => {
            let entries = struct_default_entries(&inline.fields, index, registry)?;
            if let EnumRepresentation::InternallyTagged { tag } = representation {
                let mut merged = vec![format!("{tag}: {name}")];
                merged.extend(entries);
                return Ok(format!("{{ {} }}", merged.join(", ")));
            }
            format!("{{ {} }}", entries.join(", "))
        }
        VariantData::DataStructureRef(field_type) => {
            field_type_to_default_value(field_type, index, registry)?
        }
    };
    Ok(match representation {
        EnumRepresentation::ExternallyTagged => format!("{{ {}: {payload} }}", variant.name),
        // serde writes the tag into the struct or map the variant holds.
        EnumRepresentation::InternallyTagged { tag } => {
            format!("{{ {tag}: {name}, ...{payload} }}")
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!("{{ {tag}: {name}, {content}: {payload} }}")
        }
        EnumRepresentation::Untagged => payload,
    })
}

/// A struct's fields as TypeScript default object entries.
fn struct_default_entries(
    fields: &[crate::types::StructField],
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<Vec<String>> {
    fields
        .iter()
        .map(|field| {
            Ok(format!(
                "{}: {}",
                field.field_name.to_case(Case::Camel),
                field_type_to_default_value(&field.field_type, index, registry)?
            ))
        })
        .collect()
}

/// The TypeScript default object for a struct's fields.
fn struct_fields_default(
    fields: &[crate::types::StructField],
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let entries = struct_default_entries(fields, index, registry)?;
    Ok(format!("{{ {} }}", entries.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::{FieldType, TaggedUnion, TypeIndex, field_type_to_default_value};
    use crate::types::ForeignTypeRegistry;
    use std::collections::BTreeMap;

    #[test]
    fn unit_defaults_to_null_as_serde_writes_it() {
        let default = field_type_to_default_value(
            &FieldType::Unit,
            &TypeIndex::new(&BTreeMap::new(), &BTreeMap::new()).unwrap(),
            &ForeignTypeRegistry::default(),
        )
        .unwrap();
        assert_eq!(default, "null");
    }

    #[test]
    fn an_enum_named_in_any_case_gets_its_default() {
        let role: TaggedUnion = serde_json::from_value(serde_json::json!({
            "enum_name": "Role",
            "variants": [
                { "name": "Admin", "data": null },
                { "name": "Member", "data": null, "is_default": true }
            ],
            "representation": "Untagged"
        }))
        .unwrap();
        let enums = BTreeMap::from([("Role".to_string(), role)]);
        let default = field_type_to_default_value(
            &FieldType::Other("role".to_string()),
            &TypeIndex::new(&BTreeMap::new(), &enums).unwrap(),
            &ForeignTypeRegistry::default(),
        )
        .unwrap();
        assert_eq!(default, "\"Member\"");
    }

    #[test]
    fn an_unknown_type_has_no_default() {
        let error = field_type_to_default_value(
            &FieldType::Other("Missing".to_string()),
            &TypeIndex::new(&BTreeMap::new(), &BTreeMap::new()).unwrap(),
            &ForeignTypeRegistry::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("`Missing` is neither a scanned type"),
            "{error}"
        );
    }
}
