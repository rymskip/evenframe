//! Lint pass: detect field-level annotations that are silently discarded when a
//! struct is inlined as an embedded object.
//!
//! Evenframe emits one `DEFINE FIELD` per top-level table field. When a field's
//! type is an embedded (id-less, non-table) struct, that struct is *inlined* into
//! the parent field's type as a closed literal `object { ... }`; evenframe never
//! emits per-subfield `DEFINE FIELD parent.child` statements. As a result, a
//! `#[define_field_statement(...)]` on a subfield of an embedded struct is a
//! silent no-op. This pass surfaces such annotations so the developer learns the
//! annotation is ineffective and can restructure (gate the parent field, or
//! promote the subfield to its own table).
//!
//! The one exception is `default`, which *is* honored: when building the parent's
//! `DEFAULT`, evenframe walks the embedded struct and uses each subfield's
//! `define_config.default` (see [`crate::default::field_type_to_surql_default`]).
//! [`discarded_settings`] therefore deliberately ignores `default`.

use crate::schemasync::define_config::DefineConfig;
use crate::types::StructConfig;
use std::collections::BTreeMap;

/// A `#[define_field_statement(...)]` setting on an embedded struct's field that
/// is dropped during schema generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardedAnnotation {
    pub struct_name: String,
    pub field_name: String,
    /// Names of the discarded settings, e.g. `["update_permissions", "readonly"]`.
    pub discarded: Vec<&'static str>,
}

/// Return the settings of `dc` that express intent which is dropped when the
/// owning struct is inlined as an embedded object.
///
/// `default` is excluded (it is honored). The no-annotation baseline produced by
/// [`DefineConfig::parse`] — `select`/`update`/`create` = `FULL` and
/// `flexible(false)` — is treated as neutral, so un-annotated fields yield an
/// empty list.
fn discarded_settings(dc: &DefineConfig) -> Vec<&'static str> {
    let mut out = Vec::new();
    // A permission only carries intent when it deviates from the `FULL` baseline.
    let perm_meaningful = |p: &Option<String>| matches!(p, Some(v) if v != "FULL");
    if perm_meaningful(&dc.select_permissions) {
        out.push("select_permissions");
    }
    if perm_meaningful(&dc.update_permissions) {
        out.push("update_permissions");
    }
    if perm_meaningful(&dc.create_permissions) {
        out.push("create_permissions");
    }
    if dc.readonly == Some(true) {
        out.push("readonly");
    }
    if dc.flexible == Some(true) {
        out.push("flexible");
    }
    if dc.should_skip {
        out.push("should_skip");
    }
    if dc.value.is_some() {
        out.push("value");
    }
    if dc.assert.is_some() {
        out.push("assert");
    }
    if dc.computed.is_some() {
        out.push("computed");
    }
    if dc.comment.is_some() {
        out.push("comment");
    }
    if dc.data_type.is_some() {
        out.push("data_type");
    }
    if dc.default_always.is_some() {
        out.push("default_always");
    }
    out
}

/// Walk every embedded struct and report fields whose `#[define_field_statement]`
/// carries settings that are discarded by inlining.
///
/// `objects` holds only id-less (non-table) structs, which are *always* inlined,
/// so every annotated field found here is a genuine no-op regardless of where the
/// struct is used. Iterating every entry also covers arbitrarily deep nesting and
/// structs used only as enum-variant payloads. [`StructConfig::effective`] and
/// [`crate::types::StructField::effective`] mirror emission's `output_override`
/// resolution.
pub fn lint_discarded_field_annotations(
    objects: &BTreeMap<String, StructConfig>,
) -> Vec<DiscardedAnnotation> {
    let mut findings = Vec::new();
    for object in objects.values() {
        let object = object.effective();
        for field in &object.fields {
            let field = field.effective();
            if let Some(dc) = &field.define_config {
                let discarded = discarded_settings(dc);
                if !discarded.is_empty() {
                    findings.push(DiscardedAnnotation {
                        struct_name: object.struct_name.clone(),
                        field_name: field.field_name.clone(),
                        discarded,
                    });
                }
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StructField;

    /// All-neutral `DefineConfig` to build annotated configs from via struct update.
    fn base() -> DefineConfig {
        DefineConfig {
            select_permissions: None,
            update_permissions: None,
            create_permissions: None,
            data_type: None,
            should_skip: false,
            default: None,
            default_always: None,
            value: None,
            assert: None,
            readonly: None,
            flexible: None,
            computed: None,
            comment: None,
        }
    }

    /// Mirrors the no-annotation default emitted by `DefineConfig::parse`.
    fn no_annotation_default() -> DefineConfig {
        DefineConfig {
            select_permissions: Some("FULL".to_string()),
            update_permissions: Some("FULL".to_string()),
            create_permissions: Some("FULL".to_string()),
            flexible: Some(false),
            ..base()
        }
    }

    #[test]
    fn no_annotation_default_is_neutral() {
        assert!(discarded_settings(&no_annotation_default()).is_empty());
    }

    #[test]
    fn explicit_full_permission_is_neutral() {
        let dc = DefineConfig {
            select_permissions: Some("FULL".to_string()),
            ..base()
        };
        assert!(discarded_settings(&dc).is_empty());
    }

    #[test]
    fn default_only_is_honored_not_discarded() {
        let dc = DefineConfig {
            default: Some("[]".to_string()),
            ..base()
        };
        assert!(discarded_settings(&dc).is_empty());
    }

    #[test]
    fn flexible_false_neutral_true_discarded() {
        let off = DefineConfig {
            flexible: Some(false),
            ..base()
        };
        assert!(discarded_settings(&off).is_empty());
        let on = DefineConfig {
            flexible: Some(true),
            ..base()
        };
        assert_eq!(discarded_settings(&on), vec!["flexible"]);
    }

    #[test]
    fn meaningful_permission_is_discarded() {
        let dc = DefineConfig {
            update_permissions: Some("admin".to_string()),
            ..base()
        };
        assert_eq!(discarded_settings(&dc), vec!["update_permissions"]);
    }

    #[test]
    fn readonly_and_assert_are_discarded() {
        let dc = DefineConfig {
            readonly: Some(true),
            assert: Some("$value != NONE".to_string()),
            ..base()
        };
        assert_eq!(discarded_settings(&dc), vec!["readonly", "assert"]);
    }

    fn field_with(name: &str, dc: Option<DefineConfig>) -> StructField {
        StructField {
            field_name: name.to_string(),
            define_config: dc,
            ..Default::default()
        }
    }

    #[test]
    fn lint_flags_only_discarded_annotations() {
        let mut objects = BTreeMap::new();
        objects.insert(
            "DashboardConfig".to_string(),
            StructConfig {
                struct_name: "DashboardConfig".to_string(),
                fields: vec![
                    field_with(
                        "other_layouts",
                        Some(DefineConfig {
                            update_permissions: Some("admin".to_string()),
                            ..base()
                        }),
                    ),
                    // honored: default-only -> no finding
                    field_with(
                        "name",
                        Some(DefineConfig {
                            default: Some("''".to_string()),
                            ..base()
                        }),
                    ),
                    // no-annotation baseline -> no finding
                    field_with("title", Some(no_annotation_default())),
                ],
                ..Default::default()
            },
        );

        let findings = lint_discarded_field_annotations(&objects);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].struct_name, "DashboardConfig");
        assert_eq!(findings[0].field_name, "other_layouts");
        assert_eq!(findings[0].discarded, vec!["update_permissions"]);
    }

    #[test]
    fn lint_respects_output_override() {
        // The original field's annotation looks discarded, but `output_override`
        // replaces it with a neutral field. Emission inlines the override, so the
        // lint must follow it and report nothing.
        let mut original = field_with(
            "secret",
            Some(DefineConfig {
                update_permissions: Some("admin".to_string()),
                ..base()
            }),
        );
        original.output_override =
            Some(Box::new(field_with("secret", Some(no_annotation_default()))));

        let mut objects = BTreeMap::new();
        objects.insert(
            "Embedded".to_string(),
            StructConfig {
                struct_name: "Embedded".to_string(),
                fields: vec![original],
                ..Default::default()
            },
        );

        assert!(lint_discarded_field_annotations(&objects).is_empty());
    }
}
