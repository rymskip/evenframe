//! Apply the configured TypeScript naming fallback after plugins finish.

use crate::error::{EvenframeError, Result};
use crate::types::{StructConfig, StructField, TaggedUnion, Variant, VariantData};
use crate::typesync::config::TsNames;
use std::collections::BTreeMap;

pub fn apply_struct(struct_config: &mut StructConfig, policy: TsNames) -> Result<()> {
    let emitted = struct_config.pipeline.includes_typesync() && !struct_config.resolve_only;
    apply_struct_names(struct_config, policy, emitted)
}

fn apply_struct_names(
    struct_config: &mut StructConfig,
    policy: TsNames,
    emitted: bool,
) -> Result<()> {
    if let Some(replacement) = &mut struct_config.output_override {
        apply_struct_names(replacement, policy, emitted).map_err(|error| {
            EvenframeError::type_sync(format!("naming struct override: {error}"))
        })?;
    }
    let emits_own_fields = emitted
        && (struct_config.output_override.is_none()
            || struct_config.effective().struct_name != struct_config.struct_name);
    let mut names = BTreeMap::new();
    for field in &mut struct_config.fields {
        apply_field(field, policy);
        let field = field.effective();
        if emits_own_fields && !field.wire.serde_skipped {
            let name = field.ts_name().into_owned();
            if let Some(previous) = names.insert(name.clone(), field.field_name.clone()) {
                return Err(EvenframeError::config(format!(
                    "TypeScript fields `{}.{previous}` and `{}.{}` both emit key `{name}`; use an explicit ts_name or serde rename to distinguish them",
                    struct_config.struct_name, struct_config.struct_name, field.field_name,
                )));
            }
        }
    }
    Ok(())
}

fn apply_field(field: &mut StructField, policy: TsNames) {
    if let Some(replacement) = &mut field.output_override {
        apply_field(replacement, policy);
    } else if policy == TsNames::RespectSerde && field.wire.typescript.is_none() {
        field.wire.typescript = Some(field.serde_name().to_owned());
    }
}

pub fn apply_enum(tagged_union: &mut TaggedUnion, policy: TsNames) -> Result<()> {
    let emitted = tagged_union.pipeline.includes_typesync() && !tagged_union.resolve_only;
    apply_enum_names(tagged_union, policy, emitted)
}

fn apply_enum_names(tagged_union: &mut TaggedUnion, policy: TsNames, emitted: bool) -> Result<()> {
    if let Some(replacement) = &mut tagged_union.output_override {
        apply_enum_names(replacement, policy, emitted)
            .map_err(|error| EvenframeError::type_sync(format!("naming enum override: {error}")))?;
    }
    let emits_own_payloads = emitted
        && (tagged_union.output_override.is_none()
            || tagged_union.effective().enum_name != tagged_union.enum_name);
    for variant in &mut tagged_union.variants {
        let variant_emitted = emits_own_payloads && !variant.effective().wire.serde_skipped;
        apply_variant(variant, policy, variant_emitted).map_err(|error| {
            EvenframeError::type_sync(format!("naming variant '{}': {error}", variant.name))
        })?;
    }
    Ok(())
}

fn apply_variant(variant: &mut Variant, policy: TsNames, emitted: bool) -> Result<()> {
    if let Some(replacement) = &mut variant.output_override {
        apply_variant(replacement, policy, emitted).map_err(|error| {
            EvenframeError::type_sync(format!("naming variant override: {error}"))
        })?;
    }
    if let Some(VariantData::InlineStruct(payload)) = &mut variant.data {
        apply_struct_names(payload, policy, emitted).map_err(|error| {
            EvenframeError::type_sync(format!("naming payload '{}': {error}", payload.struct_name))
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[cfg(any(feature = "arktype", feature = "effect", feature = "macroforge"))]
pub(crate) fn wire_named_structs() -> BTreeMap<String, StructConfig> {
    use crate::types::{FieldType, Wire};
    let field = |name: &str, wire: Wire| StructField {
        field_name: name.to_owned(),
        field_type: FieldType::String,
        wire,
        ..StructField::default()
    };
    let renamed = |name: &str| Wire {
        serde: Some(name.to_owned()),
        ..Wire::default()
    };
    BTreeMap::from([(
        "Person".to_owned(),
        StructConfig {
            struct_name: "Person".to_owned(),
            fields: vec![
                field("first_name", Wire::default()),
                field("last_name", renamed("lastName")),
                field("zip_code", renamed("zip-code")),
                field(
                    "nickname",
                    Wire {
                        serde_optional: true,
                        ..Wire::default()
                    },
                ),
            ],
            ..StructConfig::default()
        },
    )])
}

#[cfg(test)]
mod tests {
    use super::apply_struct;
    use crate::types::{Pipeline, StructConfig, StructField, Wire};
    use crate::typesync::config::TsNames;

    #[test]
    fn serde_policy_reaches_plugin_replacements_and_keeps_explicit_ts_names() {
        let mut config = StructConfig {
            output_override: Some(Box::new(StructConfig {
                fields: vec![
                    StructField {
                        field_name: "first_name".to_owned(),
                        ..StructField::default()
                    },
                    StructField {
                        output_override: Some(Box::new(StructField {
                            field_name: "postal_code".to_owned(),
                            wire: Wire {
                                typescript: Some("PostalCode".to_owned()),
                                ..Wire::default()
                            },
                            ..StructField::default()
                        })),
                        ..StructField::default()
                    },
                ],
                ..StructConfig::default()
            })),
            ..StructConfig::default()
        };
        apply_struct(&mut config, TsNames::RespectSerde).expect("name plugin replacements");
        assert_eq!(config.effective().fields[0].ts_name(), "first_name");
        assert_eq!(
            config.effective().fields[1].effective().ts_name(),
            "PostalCode"
        );
    }

    #[test]
    fn naming_collision_checks_follow_the_source_pipeline_through_overrides() {
        let mut config = StructConfig {
            pipeline: Pipeline::Schemasync,
            output_override: Some(Box::new(StructConfig {
                fields: ["first_name", "firstName"]
                    .into_iter()
                    .map(|name| StructField {
                        field_name: name.to_owned(),
                        ..StructField::default()
                    })
                    .collect(),
                ..StructConfig::default()
            })),
            ..StructConfig::default()
        };
        apply_struct(&mut config, TsNames::Default).expect("database keys remain distinct");
        config.pipeline = Pipeline::Typesync;
        assert!(apply_struct(&mut config, TsNames::Default).is_err());
    }

    #[test]
    fn serde_policy_names_projection_fields_and_their_redirect_target() {
        let mut config = StructConfig {
            struct_name: "PartialProfile".to_owned(),
            fields: vec![StructField {
                field_name: "first_name".to_owned(),
                ..StructField::default()
            }],
            output_override: Some(Box::new(StructConfig {
                struct_name: "Profile".to_owned(),
                fields: vec![StructField {
                    field_name: "last_name".to_owned(),
                    ..StructField::default()
                }],
                ..StructConfig::default()
            })),
            ..StructConfig::default()
        };
        apply_struct(&mut config, TsNames::RespectSerde).expect("name projected fields");
        assert_eq!(config.fields[0].ts_name(), "first_name");
        assert_eq!(config.effective().fields[0].ts_name(), "last_name");
    }
}
