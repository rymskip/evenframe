//! Struct variants whose payload is declared as a type of its own.

use crate::error::{EvenframeError, Result};
use crate::types::{FieldType, StructConfig, TaggedUnion, Variant, VariantData};
use convert_case::{Case, Casing};
use std::collections::BTreeMap;

/// Declares each struct variant's fields as a type named after the payload
/// and points the variant at it, so the payload can be used on its own. Two
/// payloads with one name and different fields, or a payload named like
/// another type, stop typesync.
pub fn declare_payloads(
    structs: &mut BTreeMap<String, StructConfig>,
    enums: &mut BTreeMap<String, TaggedUnion>,
) -> Result<()> {
    let taken: BTreeMap<String, String> = structs
        .values()
        .map(|struct_config| {
            let name = struct_config.struct_name.to_case(Case::Pascal);
            (name.clone(), format!("the struct `{name}`"))
        })
        .chain(enums.values().map(|tagged_union| {
            let name = tagged_union.enum_name.to_case(Case::Pascal);
            (name.clone(), format!("the enum `{name}`"))
        }))
        .collect();
    let mut payloads: BTreeMap<String, (String, StructConfig)> = BTreeMap::new();
    let mut problems = Vec::new();
    for tagged_union in enums.values_mut() {
        let tagged_union = effective_union_mut(tagged_union);
        let enum_name = tagged_union.enum_name.to_case(Case::Pascal);
        for variant in &mut tagged_union.variants {
            let variant = effective_variant_mut(variant);
            let Some(VariantData::InlineStruct(inline)) = &variant.data else {
                continue;
            };
            let name = inline.struct_name.to_case(Case::Pascal);
            let owner = format!("`{enum_name}::{}`", variant.name);
            match payloads.get(&name) {
                Some((first, existing)) if existing.fields != inline.effective().fields => {
                    problems.push(format!(
                        "{owner} and {first} both write their fields as the type `{name}`, \
                         with different fields"
                    ));
                }
                Some(_) => {}
                None => match taken.get(&name) {
                    Some(other) => problems.push(format!(
                        "{owner} writes its fields as the type `{name}`, which is also {other}"
                    )),
                    None => {
                        let payload = StructConfig {
                            struct_name: name.clone(),
                            ..inline.effective().clone()
                        };
                        payloads.insert(name.clone(), (owner, payload));
                    }
                },
            }
            variant.data = Some(VariantData::DataStructureRef(FieldType::Other(name)));
        }
    }
    if !problems.is_empty() {
        return Err(EvenframeError::type_sync(format!(
            "{} struct variant name {} stop typesync before it writes anything:\n{}\n\
             Rename the variants, or set struct_variants = \"inline\" in [typesync] to write \
             struct variants' fields inside their enum.",
            problems.len(),
            if problems.len() == 1 {
                "clash"
            } else {
                "clashes"
            },
            problems
                .iter()
                .map(|problem| format!("  - {problem}."))
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }
    structs.extend(
        payloads
            .into_iter()
            .map(|(name, (_, payload))| (name, payload)),
    );
    Ok(())
}

/// The union an `output_override` chain ends at, as generators read it.
fn effective_union_mut(tagged_union: &mut TaggedUnion) -> &mut TaggedUnion {
    match tagged_union.output_override {
        Some(ref mut inner) => effective_union_mut(inner),
        None => tagged_union,
    }
}

/// The variant an `output_override` chain ends at, as generators read it.
fn effective_variant_mut(variant: &mut Variant) -> &mut Variant {
    match variant.output_override {
        Some(ref mut inner) => effective_variant_mut(inner),
        None => variant,
    }
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, FieldType, StructConfig, TaggedUnion, VariantData, declare_payloads};
    use crate::types::StructField;

    fn payload(name: &str, fields: &[(&str, FieldType)]) -> VariantData {
        VariantData::InlineStruct(StructConfig {
            struct_name: name.to_string(),
            fields: fields
                .iter()
                .map(|(field_name, field_type)| StructField {
                    field_name: field_name.to_string(),
                    field_type: field_type.clone(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        })
    }

    fn union(name: &str, variants: Vec<(&str, VariantData)>) -> TaggedUnion {
        let variants: Vec<serde_json::Value> = variants
            .into_iter()
            .map(|(variant_name, data)| serde_json::json!({ "name": variant_name, "data": data }))
            .collect();
        serde_json::from_value(serde_json::json!({ "enum_name": name, "variants": variants }))
            .unwrap()
    }

    #[test]
    fn a_payload_becomes_a_named_type_the_variant_references() {
        let mut structs = BTreeMap::new();
        let mut enums = BTreeMap::from([(
            "Property".to_string(),
            union(
                "Property",
                vec![(
                    "TextValue",
                    payload("TextValue", &[("value", FieldType::String)]),
                )],
            ),
        )]);
        declare_payloads(&mut structs, &mut enums).unwrap();
        assert_eq!(structs["TextValue"].fields[0].field_name, "value");
        assert_eq!(
            enums["Property"].variants[0].data,
            Some(VariantData::DataStructureRef(FieldType::Other(
                "TextValue".to_string()
            )))
        );
    }

    #[test]
    fn identical_payloads_share_one_type() {
        let mut structs = BTreeMap::new();
        let custom = || payload("Custom", &[("connector", FieldType::String)]);
        let mut enums = BTreeMap::from([
            (
                "First".to_string(),
                union("First", vec![("Custom", custom())]),
            ),
            (
                "Second".to_string(),
                union("Second", vec![("Custom", custom())]),
            ),
        ]);
        declare_payloads(&mut structs, &mut enums).unwrap();
        assert_eq!(structs.len(), 1);
    }

    #[test]
    fn clashing_payloads_are_reported_with_the_inline_option() {
        let mut structs = BTreeMap::from([(
            "Invoice".to_string(),
            StructConfig {
                struct_name: "Invoice".to_string(),
                ..Default::default()
            },
        )]);
        let mut enums = BTreeMap::from([
            (
                "First".to_string(),
                union(
                    "First",
                    vec![
                        (
                            "Custom",
                            payload("Custom", &[("connector", FieldType::String)]),
                        ),
                        ("Invoice", payload("Invoice", &[("total", FieldType::F64)])),
                    ],
                ),
            ),
            (
                "Second".to_string(),
                union(
                    "Second",
                    vec![(
                        "Custom",
                        payload("Custom", &[("action", FieldType::String)]),
                    )],
                ),
            ),
        ]);
        let error = declare_payloads(&mut structs, &mut enums)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("2 struct variant name clashes stop typesync"),
            "{error}"
        );
        assert!(
            error.contains("`Second::Custom` and `First::Custom` both write their fields as the type `Custom`, with different fields"),
            "{error}"
        );
        assert!(
            error.contains("`First::Invoice` writes its fields as the type `Invoice`, which is also the struct `Invoice`"),
            "{error}"
        );
        assert!(error.contains("struct_variants = \"inline\""), "{error}");
    }
}
