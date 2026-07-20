//! Lint pass: detect `#[define_field_statement(...)]` settings that schema
//! generation silently discards.
//!
//! Evenframe emits one `DEFINE FIELD` per top-level table field. When a field's
//! type resolves to a struct that is *not* a materialized table, that struct is
//! inlined into the parent field's type as a closed literal `object { ... }`;
//! evenframe never emits per-subfield `DEFINE FIELD parent.child` statements, so
//! field-level settings on the inlined struct are silent no-ops. This pass
//! mirrors the emission rules exactly:
//!
//! - A struct whose effective name resolves to a materialized table is emitted
//!   as `record<table>` wherever it is referenced, and its own `DEFINE FIELD`s
//!   honor every setting — such structs are skipped entirely.
//! - A struct from a `resolve_only` include that has an `id` field is inlined
//!   (and its settings discarded) *in this run*, but the project that owns the
//!   type may materialize it as a table and honor them. That cannot be
//!   determined from this run's data, so the finding is classified
//!   [`DiscardedContext::ResolveOnlyTable`] and can be silenced via
//!   `[schemasync.lint] silence_unverifiable_annotations = true`.
//! - Any other struct in `objects` is always inlined; settings other than
//!   `default` are definitively discarded. `default` *is* honored: the parent's
//!   `DEFAULT` walk uses each subfield's `define_config.default`
//!   (see [`crate::default::field_type_to_surql_default`]).
//! - Fields of an enum variant's *inline* payload struct
//!   ([`VariantData::InlineStruct`]) are inlined into the enum's literal type.
//!   Their `default`s are honored only for the variant the `DEFAULT` walk
//!   materializes — the `#[default]`-flagged variant, else the first declared —
//!   because a parent `DEFAULT` holds exactly one variant's value; a `default`
//!   on any other variant's payload is discarded.
//!
//! Named structs referenced by enum variants ([`VariantData::DataStructureRef`])
//! are registered in `objects` and covered by the object walk; they are not
//! reported a second time from the enum walk.

use crate::schemasync::define_config::DefineConfig;
use crate::schemasync::table::TableConfig;
use crate::types::{StructConfig, TaggedUnion, VariantData};
use convert_case::{Case, Casing};
use std::collections::BTreeMap;

/// Where a discarded annotation was found, which determines both the warning
/// wording and whether the finding is definite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscardedContext {
    /// Field of a struct inlined as an embedded object — discarded in every
    /// run by construction.
    EmbeddedObject,
    /// Field of an enum variant's inline (anonymous) payload struct — always
    /// inlined. `default` is honored only on the variant the `DEFAULT` walk
    /// materializes (the `#[default]`-flagged one, else the first declared).
    EnumVariantPayload { enum_name: String },
    /// Field of a `resolve_only` struct that has an `id` field: discarded in
    /// this run, but the owning project may materialize the table and honor
    /// the settings — not knowable from this run's data.
    ResolveOnlyTable,
}

/// A `#[define_field_statement(...)]` setting that schema generation drops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscardedAnnotation {
    /// For [`DiscardedContext::EnumVariantPayload`] this is the variant name.
    pub struct_name: String,
    pub field_name: String,
    /// Names of the discarded settings, e.g. `["update_permissions", "readonly"]`.
    pub discarded: Vec<&'static str>,
    pub context: DiscardedContext,
}

/// Return the settings of `dc` that express intent which is dropped when the
/// owning struct is inlined.
///
/// `default_is_honored` is true wherever the parent's `DEFAULT` walk reads the
/// subfield's `default`: embedded objects, and the inline payload of the
/// variant the walk materializes. It is false for other variants' inline
/// payloads, which the walk never reaches. The no-annotation baseline produced by
/// [`DefineConfig::parse`] — `select`/`update`/`create` = `FULL` and
/// `flexible(false)` — is treated as neutral, so un-annotated fields yield an
/// empty list.
fn discarded_settings(dc: &DefineConfig, default_is_honored: bool) -> Vec<&'static str> {
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
    if !default_is_honored && dc.default.is_some() {
        out.push("default");
    }
    if dc.default_always.is_some() {
        out.push("default_always");
    }
    out
}

/// Walk every inlined struct — embedded objects and enum inline variant
/// payloads — and report fields whose `#[define_field_statement]` carries
/// settings that inlining discards.
///
/// `tables` is consulted to skip structs that emission resolves to
/// `record<table>` instead of inlining: type conversion looks up the
/// *effective* struct name, snake-cased, in the tables map (see the
/// `FieldType::Other` arm in `StructField`'s define-statement builder), and
/// this pass uses the identical predicate. [`StructConfig::effective`] and
/// [`crate::types::StructField::effective`] mirror emission's
/// `output_override` resolution throughout.
pub fn lint_discarded_field_annotations(
    tables: &BTreeMap<String, TableConfig>,
    objects: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
) -> Vec<DiscardedAnnotation> {
    let mut findings = Vec::new();

    for object in objects.values() {
        let effective = object.effective();
        // Emission emits `record<table>` for a struct whose effective name is a
        // materialized table; its own DEFINE FIELDs honor every setting.
        if tables.contains_key(&effective.struct_name.to_case(Case::Snake)) {
            continue;
        }
        // Table-ness is a property of the source (`id` field presence), so the
        // raw scanned fields decide what the owning project's scanner would see.
        let context = if object.resolve_only && object.fields.iter().any(|f| f.field_name == "id")
        {
            DiscardedContext::ResolveOnlyTable
        } else {
            DiscardedContext::EmbeddedObject
        };
        for field in &effective.fields {
            let field = field.effective();
            if let Some(dc) = &field.define_config {
                let discarded = discarded_settings(dc, true);
                if !discarded.is_empty() {
                    findings.push(DiscardedAnnotation {
                        struct_name: effective.struct_name.clone(),
                        field_name: field.field_name.clone(),
                        discarded,
                        context: context.clone(),
                    });
                }
            }
        }
    }

    for tagged_union in enums.values() {
        // The DEFAULT walk reads the raw union: its chosen variant is the raw
        // `is_default`-flagged one, else the first declared. Only that
        // variant's payload has its `default`s emitted.
        let chosen_variant_name = tagged_union
            .variants
            .iter()
            .find(|v| v.is_default)
            .or_else(|| tagged_union.variants.first())
            .map(|v| v.name.clone());
        let tagged_union = tagged_union.effective();
        for variant in &tagged_union.variants {
            let variant = variant.effective();
            if let Some(VariantData::InlineStruct(payload)) = &variant.data {
                let default_is_honored =
                    chosen_variant_name.as_deref() == Some(variant.name.as_str());
                // Emission uses the payload struct directly (no override
                // resolution on the payload itself), reading each field's
                // effective view — mirror that.
                for field in &payload.fields {
                    let field = field.effective();
                    if let Some(dc) = &field.define_config {
                        let discarded = discarded_settings(dc, default_is_honored);
                        if !discarded.is_empty() {
                            findings.push(DiscardedAnnotation {
                                struct_name: variant.name.clone(),
                                field_name: field.field_name.clone(),
                                discarded,
                                context: DiscardedContext::EnumVariantPayload {
                                    enum_name: tagged_union.enum_name.clone(),
                                },
                            });
                        }
                    }
                }
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EnumRepresentation, StructField, Variant};

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

    fn no_tables() -> BTreeMap<String, TableConfig> {
        BTreeMap::new()
    }

    fn no_enums() -> BTreeMap<String, TaggedUnion> {
        BTreeMap::new()
    }

    fn table_named(table_name: &str, struct_name: &str) -> TableConfig {
        TableConfig {
            table_name: table_name.to_string(),
            struct_config: StructConfig {
                struct_name: struct_name.to_string(),
                ..Default::default()
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![],
            indexes: vec![],
            output_override: None,
        }
    }

    fn field_with(name: &str, dc: Option<DefineConfig>) -> StructField {
        StructField {
            field_name: name.to_string(),
            define_config: dc,
            ..Default::default()
        }
    }

    fn plain_variant(name: &str, data: Option<VariantData>) -> Variant {
        Variant {
            name: name.to_string(),
            data,
            doccom: None,
            annotations: vec![],
            output_override: None,
            raw_attributes: BTreeMap::new(),
            is_default: false,
        }
    }

    fn union_with(enum_name: &str, variants: Vec<Variant>) -> TaggedUnion {
        TaggedUnion {
            enum_name: enum_name.to_string(),
            variants,
            representation: EnumRepresentation::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Default::default(),
            rust_derives: vec![],
            output_override: None,
            resolve_only: false,
            raw_attributes: BTreeMap::new(),
        }
    }

    #[test]
    fn no_annotation_default_is_neutral() {
        assert!(discarded_settings(&no_annotation_default(), true).is_empty());
        assert!(discarded_settings(&no_annotation_default(), false).is_empty());
    }

    #[test]
    fn explicit_full_permission_is_neutral() {
        let dc = DefineConfig {
            select_permissions: Some("FULL".to_string()),
            ..base()
        };
        assert!(discarded_settings(&dc, true).is_empty());
    }

    #[test]
    fn default_only_discarded_where_the_default_walk_cannot_reach_it() {
        let dc = DefineConfig {
            default: Some("[]".to_string()),
            ..base()
        };
        assert!(discarded_settings(&dc, true).is_empty());
        assert_eq!(discarded_settings(&dc, false), vec!["default"]);
    }

    #[test]
    fn flexible_false_neutral_true_discarded() {
        let off = DefineConfig {
            flexible: Some(false),
            ..base()
        };
        assert!(discarded_settings(&off, true).is_empty());
        let on = DefineConfig {
            flexible: Some(true),
            ..base()
        };
        assert_eq!(discarded_settings(&on, true), vec!["flexible"]);
    }

    #[test]
    fn meaningful_permission_is_discarded() {
        let dc = DefineConfig {
            update_permissions: Some("admin".to_string()),
            ..base()
        };
        assert_eq!(discarded_settings(&dc, true), vec!["update_permissions"]);
    }

    #[test]
    fn readonly_and_assert_are_discarded() {
        let dc = DefineConfig {
            readonly: Some(true),
            assert: Some("$value != NONE".to_string()),
            ..base()
        };
        assert_eq!(discarded_settings(&dc, true), vec!["readonly", "assert"]);
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

        let findings = lint_discarded_field_annotations(&no_tables(), &objects, &no_enums());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].struct_name, "DashboardConfig");
        assert_eq!(findings[0].field_name, "other_layouts");
        assert_eq!(findings[0].discarded, vec!["update_permissions"]);
        assert_eq!(findings[0].context, DiscardedContext::EmbeddedObject);
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

        assert!(
            lint_discarded_field_annotations(&no_tables(), &objects, &no_enums()).is_empty()
        );
    }

    #[test]
    fn lint_skips_structs_materialized_as_tables() {
        // A table struct's DEFINE FIELDs honor every setting, and references to
        // it emit record<> — never a warning.
        let mut objects = BTreeMap::new();
        objects.insert(
            "UserProfile".to_string(),
            StructConfig {
                struct_name: "UserProfile".to_string(),
                fields: vec![
                    field_with("id", Some(no_annotation_default())),
                    field_with(
                        "email",
                        Some(DefineConfig {
                            update_permissions: Some("WHERE $auth.id = id".to_string()),
                            assert: Some("string::is::email($value)".to_string()),
                            readonly: Some(true),
                            ..base()
                        }),
                    ),
                ],
                ..Default::default()
            },
        );
        let mut tables = BTreeMap::new();
        tables.insert(
            "user_profile".to_string(),
            table_named("user_profile", "UserProfile"),
        );

        assert!(lint_discarded_field_annotations(&tables, &objects, &no_enums()).is_empty());
    }

    #[test]
    fn lint_follows_override_to_table() {
        // A synthetic projection whose effective struct is a table is emitted
        // as record<> (never inlined), so its annotations must not be flagged.
        let mut projection = StructConfig {
            struct_name: "PartialUser".to_string(),
            fields: vec![field_with(
                "email",
                Some(DefineConfig {
                    readonly: Some(true),
                    ..base()
                }),
            )],
            ..Default::default()
        };
        projection.output_override = Some(Box::new(StructConfig {
            struct_name: "User".to_string(),
            fields: projection.fields.clone(),
            ..Default::default()
        }));

        let mut objects = BTreeMap::new();
        objects.insert("PartialUser".to_string(), projection);
        let mut tables = BTreeMap::new();
        tables.insert("user".to_string(), table_named("user", "User"));

        assert!(lint_discarded_field_annotations(&tables, &objects, &no_enums()).is_empty());
    }

    #[test]
    fn resolve_only_with_id_is_unverifiable() {
        let mut objects = BTreeMap::new();
        objects.insert(
            "ExternalUser".to_string(),
            StructConfig {
                struct_name: "ExternalUser".to_string(),
                resolve_only: true,
                fields: vec![
                    field_with("id", Some(no_annotation_default())),
                    field_with(
                        "email",
                        Some(DefineConfig {
                            readonly: Some(true),
                            ..base()
                        }),
                    ),
                ],
                ..Default::default()
            },
        );

        let findings = lint_discarded_field_annotations(&no_tables(), &objects, &no_enums());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].context, DiscardedContext::ResolveOnlyTable);
        assert_eq!(findings[0].discarded, vec!["readonly"]);
    }

    #[test]
    fn resolve_only_without_id_is_definite() {
        let mut objects = BTreeMap::new();
        objects.insert(
            "ExternalShape".to_string(),
            StructConfig {
                struct_name: "ExternalShape".to_string(),
                resolve_only: true,
                fields: vec![field_with(
                    "kind",
                    Some(DefineConfig {
                        readonly: Some(true),
                        ..base()
                    }),
                )],
                ..Default::default()
            },
        );

        let findings = lint_discarded_field_annotations(&no_tables(), &objects, &no_enums());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].context, DiscardedContext::EmbeddedObject);
    }

    fn payload_with_assert_and_default(variant_name: &str) -> StructConfig {
        StructConfig {
            struct_name: variant_name.to_string(),
            fields: vec![field_with(
                "threshold",
                Some(DefineConfig {
                    assert: Some("$value > 0".to_string()),
                    default: Some("10".to_string()),
                    ..base()
                }),
            )],
            ..Default::default()
        }
    }

    #[test]
    fn enum_inline_payload_chosen_variant_default_is_honored() {
        // The only variant is the DEFAULT walk's chosen one, so its payload
        // `default` is emitted; only `assert` is discarded.
        let mut enums = BTreeMap::new();
        enums.insert(
            "Strategy".to_string(),
            union_with(
                "Strategy",
                vec![plain_variant(
                    "Custom",
                    Some(VariantData::InlineStruct(payload_with_assert_and_default(
                        "Custom",
                    ))),
                )],
            ),
        );

        let findings =
            lint_discarded_field_annotations(&no_tables(), &BTreeMap::new(), &enums);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].struct_name, "Custom");
        assert_eq!(findings[0].field_name, "threshold");
        assert_eq!(findings[0].discarded, vec!["assert"]);
        assert_eq!(
            findings[0].context,
            DiscardedContext::EnumVariantPayload {
                enum_name: "Strategy".to_string()
            }
        );
    }

    #[test]
    fn enum_inline_payload_nonchosen_variant_default_is_discarded() {
        // No variant carries `#[default]`, so the walk materializes the first
        // declared variant; the second variant's payload `default` never lands.
        let mut enums = BTreeMap::new();
        enums.insert(
            "Strategy".to_string(),
            union_with(
                "Strategy",
                vec![
                    plain_variant("Fixed", None),
                    plain_variant(
                        "Custom",
                        Some(VariantData::InlineStruct(payload_with_assert_and_default(
                            "Custom",
                        ))),
                    ),
                ],
            ),
        );

        let findings =
            lint_discarded_field_annotations(&no_tables(), &BTreeMap::new(), &enums);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].discarded, vec!["assert", "default"]);
    }

    #[test]
    fn enum_inline_payload_default_marked_variant_wins_over_first() {
        // `#[default]` on the payload variant makes it the chosen one even when
        // declared last — its `default` is honored.
        let mut chosen = plain_variant(
            "Custom",
            Some(VariantData::InlineStruct(payload_with_assert_and_default(
                "Custom",
            ))),
        );
        chosen.is_default = true;
        let mut enums = BTreeMap::new();
        enums.insert(
            "Strategy".to_string(),
            union_with("Strategy", vec![plain_variant("Fixed", None), chosen]),
        );

        let findings =
            lint_discarded_field_annotations(&no_tables(), &BTreeMap::new(), &enums);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].discarded, vec!["assert"]);
    }

    #[test]
    fn enum_data_structure_ref_is_reported_once_via_objects() {
        // A named payload struct is registered in `objects`; the enum walk must
        // not report it a second time.
        let mut objects = BTreeMap::new();
        objects.insert(
            "Payload".to_string(),
            StructConfig {
                struct_name: "Payload".to_string(),
                fields: vec![field_with(
                    "note",
                    Some(DefineConfig {
                        comment: Some("internal".to_string()),
                        ..base()
                    }),
                )],
                ..Default::default()
            },
        );
        let mut enums = BTreeMap::new();
        enums.insert(
            "Wrapper".to_string(),
            union_with(
                "Wrapper",
                vec![plain_variant(
                    "Data",
                    Some(VariantData::DataStructureRef(crate::types::FieldType::Other(
                        "Payload".to_string(),
                    ))),
                )],
            ),
        );

        let findings = lint_discarded_field_annotations(&no_tables(), &objects, &enums);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].struct_name, "Payload");
        assert_eq!(findings[0].context, DiscardedContext::EmbeddedObject);
    }
}
