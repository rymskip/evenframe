//! End-to-end test for the workspace scanner picking up struct-level
//! `#[index(...)]` attributes and threading them through `TableConfig` so
//! that `generate_define_statements` emits real `DEFINE INDEX` lines.
//!
//! This test exists because every other index-related test in the tree
//! either bypasses the scanner (hand-built `TableConfig` literals, JSON
//! fixtures fed to insta) or stops at compile time (trybuild). The CLI
//! actually invokes the scanner pipeline, so the only test that proves
//! the feature works for real users has to drive that same pipeline.

#![cfg(feature = "schemasync")]

use evenframe_core::schemasync::compare::{Comparator, SchemaDefinition};
use evenframe_core::schemasync::database::surql::define::generate_define_statements;
use evenframe_core::schemasync::database::surql::remove::generate_remove_index_statements;
use evenframe_core::tooling::{BuildConfig, build_all_configs};
use evenframe_core::types::ForeignTypeRegistry;
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

fn write(tmp: &TempDir, rel: &str, body: &str) {
    let p = tmp.path().join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, body).unwrap();
}

#[test]
fn scanner_threads_struct_level_index_into_define_statements() {
    let tmp = TempDir::new().unwrap();

    write(
        &tmp,
        "Cargo.toml",
        r#"
            [package]
            name = "scanner_index_fixture"
            version = "0.0.0"
            edition = "2024"
        "#,
    );

    write(
        &tmp,
        "src/lib.rs",
        r#"
            #[derive(Evenframe)]
            #[index(fields(user, message), unique)]
            #[index(fields(created_at))]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
                pub emoji: String,
                pub created_at: String,
            }
        "#,
    );

    let config = BuildConfig {
        scan_path: tmp.path().to_path_buf(),
        ..BuildConfig::default()
    };

    let (_enums, tables, _objects) = build_all_configs(&config).expect("build_all_configs");

    let table = tables
        .get("reaction")
        .expect("scanner did not produce a `reaction` TableConfig");

    assert_eq!(
        table.indexes.len(),
        2,
        "expected scanner to populate 2 indexes from #[index(...)] attrs, got {:?}",
        table.indexes,
    );

    let registry = ForeignTypeRegistry::default();
    // `generate_define_statements` invokes `evenframe_log!`, which under the
    // `dev-mode` feature (enabled by `--all-features`) requires
    // `ABSOLUTE_PATH_TO_EVENFRAME` to be set. Scope the var to this call so
    // parallel tests are unaffected.
    let surql = temp_env::with_var(
        "ABSOLUTE_PATH_TO_EVENFRAME",
        Some(tmp.path().to_str().unwrap()),
        || {
            generate_define_statements(
                "reaction",
                table,
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                false,
                &registry,
            )
        },
    );

    assert!(
        surql.contains(
            "DEFINE INDEX OVERWRITE idx_reaction_user_message ON TABLE reaction \
             FIELDS user, message UNIQUE;"
        ),
        "missing composite UNIQUE index in scanner-driven SurrealQL:\n{}",
        surql,
    );
    assert!(
        surql.contains(
            "DEFINE INDEX OVERWRITE idx_reaction_created_at ON TABLE reaction FIELDS created_at;"
        ),
        "missing single-column non-unique index in scanner-driven SurrealQL:\n{}",
        surql,
    );
}

#[test]
fn scanner_rejects_unknown_field_in_index() {
    let tmp = TempDir::new().unwrap();

    write(
        &tmp,
        "Cargo.toml",
        r#"
            [package]
            name = "scanner_index_bad_fixture"
            version = "0.0.0"
            edition = "2024"
        "#,
    );

    write(
        &tmp,
        "src/lib.rs",
        r#"
            #[derive(Evenframe)]
            #[index(fields(nonexistent))]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
            }
        "#,
    );

    let config = BuildConfig {
        scan_path: tmp.path().to_path_buf(),
        ..BuildConfig::default()
    };

    let err = build_all_configs(&config)
        .expect_err("scanner should reject #[index(fields(nonexistent))]");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown field `nonexistent`"),
        "expected `unknown field` error, got: {}",
        msg,
    );
}

/// Drive the full user-facing pipeline (scanner → TableConfig →
/// SchemaDefinition → Comparator → remove generator) and assert that an index
/// which was present in the "previous" schema but removed from the Rust source
/// produces a `REMOVE INDEX` statement. Without this wiring, orphan indexes
/// would leak into the DB indefinitely.
#[test]
fn orphan_index_is_dropped_when_removed_from_source() {
    // Pass 1: both indexes declared.
    let tmp_before = TempDir::new().unwrap();
    write(
        &tmp_before,
        "Cargo.toml",
        r#"
            [package]
            name = "scanner_index_before_fixture"
            version = "0.0.0"
            edition = "2024"
        "#,
    );
    write(
        &tmp_before,
        "src/lib.rs",
        r#"
            #[derive(Evenframe)]
            #[index(fields(user, message), unique)]
            #[index(fields(created_at))]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
                pub emoji: String,
                pub created_at: String,
            }
        "#,
    );
    let before_cfg = BuildConfig {
        scan_path: tmp_before.path().to_path_buf(),
        ..BuildConfig::default()
    };
    let (_e1, before_tables, _o1) = build_all_configs(&before_cfg).expect("build before");
    let before_schema =
        SchemaDefinition::from_table_configs(&before_tables).expect("schema before");

    // Pass 2: `created_at` index removed from the struct.
    let tmp_after = TempDir::new().unwrap();
    write(
        &tmp_after,
        "Cargo.toml",
        r#"
            [package]
            name = "scanner_index_after_fixture"
            version = "0.0.0"
            edition = "2024"
        "#,
    );
    write(
        &tmp_after,
        "src/lib.rs",
        r#"
            #[derive(Evenframe)]
            #[index(fields(user, message), unique)]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
                pub emoji: String,
                pub created_at: String,
            }
        "#,
    );
    let after_cfg = BuildConfig {
        scan_path: tmp_after.path().to_path_buf(),
        ..BuildConfig::default()
    };
    let (_e2, after_tables, _o2) = build_all_configs(&after_cfg).expect("build after");
    let after_schema = SchemaDefinition::from_table_configs(&after_tables).expect("schema after");

    // Compare "old" (before) vs "new" (after) — simulates a database whose
    // indexes were last synced under the old schema.
    let changes = Comparator::compare(&before_schema, &after_schema).expect("compare");

    let table_change = changes
        .modified_tables
        .iter()
        .find(|t| t.table_name == "reaction")
        .expect("reaction table should be flagged as modified");
    assert_eq!(
        table_change.removed_indexes.len(),
        1,
        "expected exactly one removed index, got {:?}",
        table_change.removed_indexes,
    );
    assert_eq!(
        table_change.removed_indexes[0].name,
        "idx_reaction_created_at"
    );

    let remove_sql = generate_remove_index_statements(&changes);
    assert!(
        remove_sql.contains("REMOVE INDEX IF EXISTS idx_reaction_created_at ON TABLE reaction;"),
        "missing REMOVE INDEX in generated SurrealQL:\n{}",
        remove_sql,
    );
    assert!(
        !remove_sql.contains("idx_reaction_user_message"),
        "unique index should be preserved, not dropped:\n{}",
        remove_sql,
    );
}
