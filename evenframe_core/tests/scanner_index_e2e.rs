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

use evenframe_core::schemasync::database::surql::define::generate_define_statements;
use evenframe_core::tooling::{BuildConfig, build_all_configs};
use evenframe_core::types::ForeignTypeRegistry;
use std::collections::HashMap;
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
    let surql = generate_define_statements(
        "reaction",
        table,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        false,
        &registry,
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
