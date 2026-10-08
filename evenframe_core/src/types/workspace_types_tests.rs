use super::{merge_foreign_types, merge_project_types};
use crate::config::ForeignTypeConfig;
use crate::types::{AllConfigs, NewtypeConfig, StructConfig};
use std::collections::BTreeMap;

fn object(name: &str, doccom: &str, resolve_only: bool) -> StructConfig {
    StructConfig {
        struct_name: name.to_string(),
        doccom: Some(doccom.to_string()),
        resolve_only,
        ..Default::default()
    }
}

fn with_object(object: StructConfig) -> AllConfigs {
    let mut configs = AllConfigs::default();
    configs.objects.insert(object.struct_name.clone(), object);
    configs
}

#[test]
fn a_type_both_projects_emit_alike_is_kept_once() {
    let merged = merge_project_types([
        (
            "core",
            with_object(object("Address", "a postal address", false)),
        ),
        (
            "idp",
            with_object(object("Address", "a postal address", false)),
        ),
    ])
    .expect("merges");

    assert_eq!(merged.objects.len(), 1);
}

#[test]
fn the_project_that_emits_a_type_wins_over_one_that_only_resolves_it() {
    let merged = merge_project_types([
        (
            "idp",
            with_object(object("Address", "as idp reads it", true)),
        ),
        (
            "core",
            with_object(object("Address", "as core emits it", false)),
        ),
    ])
    .expect("merges");

    let address = &merged.objects["Address"];
    assert!(!address.resolve_only);
    assert_eq!(address.doccom.as_deref(), Some("as core emits it"));
}

#[test]
fn a_resolve_only_copy_never_replaces_an_emitted_one() {
    let merged = merge_project_types([
        (
            "core",
            with_object(object("Address", "as core emits it", false)),
        ),
        (
            "idp",
            with_object(object("Address", "as idp reads it", true)),
        ),
    ])
    .expect("merges");

    assert_eq!(
        merged.objects["Address"].doccom.as_deref(),
        Some("as core emits it")
    );
}

#[test]
fn two_projects_emitting_different_definitions_is_an_error() {
    let error = merge_project_types([
        ("core", with_object(object("Address", "one shape", false))),
        (
            "idp",
            with_object(object("Address", "another shape", false)),
        ),
    ])
    .expect_err("the definitions differ")
    .to_string();

    assert!(
        error.contains("The struct `Address` is emitted by both `core` and `idp`"),
        "{error}"
    );
}

#[test]
fn newtypes_merge_by_the_same_rule() {
    let newtype = |doccom: &str, resolve_only| NewtypeConfig {
        name: "NonEmptyString".to_string(),
        doccom: Some(doccom.to_string()),
        resolve_only,
        ..Default::default()
    };
    let mut core = AllConfigs::default();
    core.newtypes
        .insert("NonEmptyString".to_string(), newtype("core's", false));
    let mut idp = AllConfigs::default();
    idp.newtypes
        .insert("NonEmptyString".to_string(), newtype("idp's", false));

    let error = merge_project_types([("core", core), ("idp", idp)])
        .expect_err("both emit it, differently")
        .to_string();

    assert!(error.contains("The newtype `NonEmptyString`"), "{error}");
}

fn foreign(
    surrealdb_non_id_format: &str,
    macroforge_type: &str,
) -> BTreeMap<String, ForeignTypeConfig> {
    let config: ForeignTypeConfig = toml::from_str(&format!(
        r#"
rust_type_names = ["RecordId"]
surrealdb_non_id_format = "{surrealdb_non_id_format}"
macroforge = {{ type = "{macroforge_type}" }}
"#
    ))
    .expect("a foreign type");
    BTreeMap::from([("RecordId".to_string(), config)])
}

#[test]
fn projects_may_map_a_foreign_type_to_different_database_types() {
    let core = foreign("record", "RecordId");
    let idp = foreign("record<any>", "RecordId");

    let merged = merge_foreign_types([("core", &core), ("idp", &idp)]).expect("merges");

    assert_eq!(merged.len(), 1);
}

#[test]
fn projects_may_not_map_a_foreign_type_to_different_typescript() {
    let core = foreign("record", "RecordId");
    let idp = foreign("record", "string");

    let error = merge_foreign_types([("core", &core), ("idp", &idp)])
        .expect_err("two TypeScript types")
        .to_string();

    assert!(
        error.contains("foreign_types.RecordId maps to different TypeScript in `core` and `idp`"),
        "{error}"
    );
}

#[test]
fn a_table_two_projects_sync_may_differ_in_what_only_the_database_reads() {
    use crate::schemasync::TableConfig;
    let table =
        |permissions: Option<crate::schemasync::permissions::PermissionsConfig>| TableConfig {
            table_name: "provider".to_string(),
            struct_config: object("Provider", "a provider", false),
            relation: None,
            permissions,
            mock_generation_config: None,
            events: Vec::new(),
            indexes: Vec::new(),
            output_override: None,
        };
    let mut core = AllConfigs::default();
    core.tables.insert("provider".to_string(), table(None));
    let mut idp = AllConfigs::default();
    idp.tables.insert(
        "provider".to_string(),
        table(Some(crate::schemasync::permissions::PermissionsConfig {
            all_permissions: Some("FULL".to_string()),
            select_permissions: None,
            update_permissions: None,
            delete_permissions: None,
            create_permissions: None,
        })),
    );

    let merged = merge_project_types([("core", core), ("idp", idp)]).expect("merges");

    assert_eq!(merged.tables.len(), 1);
}
