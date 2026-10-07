//! End-to-end test for the workspace scanner picking up struct-level
//! `#[indexes(...)]` entries and threading them through `TableConfig` so
//! that `generate_define_statements` emits real `DEFINE INDEX` lines.
//!
//! This test exists because every other index-related test in the tree
//! either bypasses the scanner (hand-built `TableConfig` literals, JSON
//! fixtures fed to insta) or stops at compile time (trybuild). The CLI
//! actually invokes the scanner pipeline, so the only test that proves
//! the feature works for real users has to drive that same pipeline.

#![cfg(all(feature = "schemasync", feature = "scan"))]

use evenframe_core::scan::{ScanConfig, build_all_configs};
use evenframe_core::schemasync::compare::surql::{SchemaImporter, export_schema};
use evenframe_core::schemasync::compare::{Comparator, SchemaDefinition};
use evenframe_core::schemasync::database::surql::define::generate_define_statements;
use evenframe_core::schemasync::database::surql::remove::generate_remove_index_statements;
use evenframe_core::types::AllConfigs;
use evenframe_core::types::ForeignTypeRegistry;
use std::collections::BTreeMap;
use std::fs;
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
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
            #[indexes(
                reaction_user_message(fields(user, message), unique),
                reaction_created_at(fields(created_at)),
            )]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
                pub emoji: String,
                pub created_at: String,
            }
        "#,
    );

    let config = ScanConfig {
        scan_path: tmp.path().to_path_buf(),
        ..ScanConfig::default()
    };

    let tables = build_all_configs(&config)
        .expect("build_all_configs")
        .tables;

    let table = tables
        .get("reaction")
        .expect("scanner did not produce a `reaction` TableConfig");

    assert_eq!(
        table.indexes.len(),
        2,
        "expected scanner to populate 2 indexes from #[indexes(...)] entries, got {:?}",
        table.indexes,
    );

    let registry = ForeignTypeRegistry::default();
    let surql = generate_define_statements(
        "reaction",
        table,
        &evenframe_core::schemasync::database::surql::shape::DefineContext {
            tables: &BTreeMap::new(),
            objects: &BTreeMap::new(),
            enums: &BTreeMap::new(),
            declared: &evenframe_core::types::DeclaredTypes::default(),
            registry: &registry,
            options: evenframe_core::schemasync::config::SurqlOptions::from(true),
        },
    )
    .unwrap();

    assert!(
        surql.contains(
            "DEFINE INDEX OVERWRITE reaction_user_message ON TABLE reaction \
             FIELDS user, message UNIQUE;"
        ),
        "missing composite UNIQUE index in scanner-driven SurrealQL:\n{}",
        surql,
    );
    assert!(
        surql.contains(
            "DEFINE INDEX OVERWRITE reaction_created_at ON TABLE reaction FIELDS created_at;"
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
            #[indexes(bad(fields(nonexistent)))]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
            }
        "#,
    );

    let config = ScanConfig {
        scan_path: tmp.path().to_path_buf(),
        ..ScanConfig::default()
    };

    let err = build_all_configs(&config)
        .expect_err("scanner should reject #[indexes(bad(fields(nonexistent)))]");
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
#[tokio::test]
async fn orphan_index_is_dropped_when_removed_from_source() {
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
            #[indexes(
                reaction_user_message(fields(user, message), unique),
                reaction_created_at(fields(created_at)),
            )]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
                pub emoji: String,
                pub created_at: String,
            }
        "#,
    );
    let before_cfg = ScanConfig {
        scan_path: tmp_before.path().to_path_buf(),
        ..ScanConfig::default()
    };
    let before_schema = generated_schema(&before_cfg).await;

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
            #[indexes(reaction_user_message(fields(user, message), unique))]
            pub struct Reaction {
                pub id: String,
                pub user: String,
                pub message: String,
                pub emoji: String,
                pub created_at: String,
            }
        "#,
    );
    let after_cfg = ScanConfig {
        scan_path: tmp_after.path().to_path_buf(),
        ..ScanConfig::default()
    };
    let after_schema = generated_schema(&after_cfg).await;

    // Compare "old" (before) vs "new" (after), as for a database whose
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
    assert_eq!(table_change.removed_indexes[0].name, "reaction_created_at");

    let remove_sql = generate_remove_index_statements(&changes);
    assert!(
        remove_sql.contains("REMOVE INDEX IF EXISTS reaction_created_at ON TABLE reaction;"),
        "missing REMOVE INDEX in generated SurrealQL:\n{}",
        remove_sql,
    );
    assert!(
        !remove_sql.contains("reaction_user_message"),
        "unique index should be preserved, not dropped:\n{}",
        remove_sql,
    );
}

/// The schema the scanned project generates, applied to an in-memory
/// database and read back from its export, as schemasync reads the schema it
/// compares against the database.
async fn generated_schema(config: &ScanConfig) -> SchemaDefinition {
    let types = build_all_configs(config)
        .expect("the project scans")
        .into_schemasync()
        .expect("the schemasync view");
    let surql = evenframe_core::schemasync::dump::tables_surql(
        &types,
        &ForeignTypeRegistry::default(),
        true,
    )
    .expect("the schema generates");
    let db = Surreal::new::<Mem>(())
        .await
        .expect("an in-memory database");
    db.use_ns("test")
        .use_db("test")
        .await
        .expect("the database opens");
    db.query(surql.as_str())
        .await
        .expect("the schema runs")
        .check()
        .expect("every DEFINE succeeds");
    let export = export_schema(&db, "the generated schema")
        .await
        .expect("the schema exports");
    SchemaImporter::parse_schema_from_export(&export).expect("the export reads back")
}

fn scan_single_file(name: &str, source: &str) -> evenframe_core::error::Result<AllConfigs> {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp,
        "Cargo.toml",
        &format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n"),
    );
    write(&tmp, "src/lib.rs", source);
    let config = ScanConfig {
        scan_path: tmp.path().to_path_buf(),
        ..ScanConfig::default()
    };
    build_all_configs(&config)
}

#[test]
fn scanner_collects_field_level_indexes() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp,
        "Cargo.toml",
        "[package]\nname = \"scanner_field_index_fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    );
    write(
        &tmp,
        "src/lib.rs",
        r#"
            #[derive(Evenframe)]
            #[indexes(post_created_at(fields(created_at)))]
            pub struct Post {
                pub id: String,
                #[fulltext(analyzer = "en", bm25)]
                pub body: String,
                #[hnsw(dimension = 3)]
                #[diskann(dimension = 3)]
                pub embedding: Vec<f32>,
                pub created_at: String,
            }
        "#,
    );
    let config = ScanConfig {
        scan_path: tmp.path().to_path_buf(),
        ..ScanConfig::default()
    };
    let tables = build_all_configs(&config)
        .expect("build_all_configs")
        .tables;
    let names: Vec<String> = tables["post"]
        .indexes
        .iter()
        .map(|i| i.index_name("post"))
        .collect();
    assert_eq!(
        names,
        vec![
            "post_created_at",
            "idx_post_body_fulltext",
            "idx_post_embedding_hnsw",
            "idx_post_embedding_diskann",
        ]
    );
}

#[test]
fn named_field_unique_replaces_default_unique_index() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp,
        "Cargo.toml",
        "[package]\nname = \"scanner_named_unique_fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    );
    write(
        &tmp,
        "src/lib.rs",
        r#"
            #[derive(Evenframe)]
            pub struct Account {
                pub id: String,
                #[unique(name = "account_email", comment = "login", concurrently)]
                pub email: String,
                #[unique]
                pub handle: String,
            }
        "#,
    );
    let config = ScanConfig {
        scan_path: tmp.path().to_path_buf(),
        ..ScanConfig::default()
    };
    let tables = build_all_configs(&config)
        .expect("build_all_configs")
        .tables;
    let account = &tables["account"];
    assert!(
        account
            .struct_config
            .fields
            .iter()
            .find(|f| f.field_name == "email")
            .unwrap()
            .unique,
        "#[unique(...)] must still mark the field unique"
    );

    let surql = generate_define_statements(
        "account",
        account,
        &evenframe_core::schemasync::database::surql::shape::DefineContext {
            tables: &BTreeMap::new(),
            objects: &BTreeMap::new(),
            enums: &BTreeMap::new(),
            declared: &evenframe_core::types::DeclaredTypes::default(),
            registry: &ForeignTypeRegistry::default(),
            options: evenframe_core::schemasync::config::SurqlOptions::from(true),
        },
    )
    .unwrap();
    let index_lines: Vec<&str> = surql
        .lines()
        .filter(|l| l.starts_with("DEFINE INDEX"))
        .collect();
    assert_eq!(
        index_lines,
        vec![
            "DEFINE INDEX OVERWRITE account_email ON TABLE account FIELDS email UNIQUE COMMENT 'login' CONCURRENTLY;",
            "DEFINE INDEX OVERWRITE idx_account_handle ON TABLE account FIELDS handle UNIQUE;",
        ]
    );
}

#[test]
fn scanner_rejects_single_field_struct_level_unique() {
    let err = scan_single_file(
        "scanner_single_unique_fixture",
        r#"
            #[derive(Evenframe)]
            #[indexes(email(fields(email), unique))]
            pub struct Account { pub id: String, pub email: String }
        "#,
    )
    .expect_err("single-field struct-level unique must be rejected");
    assert!(
        err.to_string().contains("use `#[unique]` on `email`"),
        "unexpected error: {err}"
    );
}

#[test]
fn scanner_rejects_struct_level_fulltext_on_a_whole_field() {
    let err = scan_single_file(
        "scanner_struct_fulltext_fixture",
        r#"
            #[derive(Evenframe)]
            #[indexes(search(fields(body), fulltext(analyzer = "en")))]
            pub struct Post { pub id: String, pub body: String }
        "#,
    )
    .expect_err("struct-level fulltext on a whole field must be rejected");
    assert!(
        err.to_string().contains("use `#[fulltext(...)]` on `body`"),
        "unexpected error: {err}"
    );
}

#[test]
fn scanner_rejects_any_index_inside_an_optional_field() {
    let err = scan_single_file(
        "scanner_optional_path_fixture",
        r#"
            #[derive(Evenframe)]
            pub struct Contact { pub email: String }

            #[derive(Evenframe)]
            #[indexes(contact_email(fields("contact.email"), unique))]
            pub struct Account { pub id: String, pub contact: Option<Contact> }
        "#,
    )
    .expect_err("an index inside an optional field must be rejected");
    assert!(
        err.to_string()
            .contains("can't index a path inside an optional field: `contact`"),
        "unexpected error: {err}"
    );
}

#[test]
fn scanner_accepts_struct_level_indexes_on_nested_paths() {
    let AllConfigs {
        tables, objects, ..
    } = scan_single_file(
        "scanner_nested_path_fixture",
        r#"
            #[derive(Evenframe)]
            pub struct Author { pub name: String, pub embedding: Vec<f32> }

            #[derive(Evenframe)]
            #[indexes(
                author_name_search(fields("author.name"), fulltext(analyzer = "en", bm25)),
                author_ann(fields("author.embedding"), diskann(dimension = 3), concurrently),
            )]
            pub struct Post { pub id: String, pub author: Author }
        "#,
    )
    .expect("nested-path indexes must be accepted");
    let post = &tables["post"];
    let statements: Vec<String> = post
        .all_indexes("post", &objects)
        .iter()
        .map(|index| index.define_statement("post"))
        .collect();
    assert_eq!(
        statements,
        vec![
            "DEFINE INDEX OVERWRITE author_name_search ON TABLE post FIELDS author.name FULLTEXT ANALYZER en BM25;",
            "DEFINE INDEX OVERWRITE author_ann ON TABLE post FIELDS author.embedding DISKANN DIMENSION 3 CONCURRENTLY;",
        ]
    );
}

#[test]
fn scanner_rejects_colliding_index_names() {
    let err = scan_single_file(
        "scanner_index_collision_fixture",
        r#"
            #[derive(Evenframe)]
            pub struct Post {
                pub id: String,
                #[hnsw(dimension = 3, name = "post_vector")]
                #[diskann(dimension = 3, name = "post_vector")]
                pub embedding: Vec<f32>,
            }
        "#,
    )
    .expect_err("two indexes named alike must be rejected");
    assert!(
        err.to_string()
            .contains("already uses the name 'post_vector'"),
        "unexpected error: {err}"
    );
}

#[test]
fn scanner_rejects_the_old_index_attribute() {
    let err = scan_single_file(
        "scanner_old_index_fixture",
        r#"
            #[derive(Evenframe)]
            #[index(fields(user, message), unique)]
            pub struct Reaction { pub id: String, pub user: String, pub message: String }
        "#,
    )
    .expect_err("#[index(...)] must be rejected");
    assert!(
        err.to_string()
            .contains("was replaced by a single `#[indexes(...)]`"),
        "unexpected error: {err}"
    );
}

#[test]
fn scanner_rejects_two_indexes_of_a_kind_on_one_field() {
    let err = scan_single_file(
        "scanner_same_kind_fixture",
        r#"
            #[derive(Evenframe)]
            pub struct Post {
                pub id: String,
                #[hnsw(dimension = 3)]
                #[hnsw(dimension = 3, m = 8)]
                pub embedding: Vec<f32>,
            }
        "#,
    )
    .expect_err("a second #[hnsw] on one field must be rejected");
    assert!(
        err.to_string()
            .contains("only one #[hnsw] is allowed per field"),
        "unexpected error: {err}"
    );
}

#[test]
fn nested_index_paths_name_each_field_as_the_database_stores_it() {
    let AllConfigs {
        tables, objects, ..
    } = scan_single_file(
        "scanner_renamed_nested_path_fixture",
        r#"
            #[derive(Evenframe)]
            #[surreal(rename_all = "camelCase")]
            pub struct Author { pub display_name: String }

            #[derive(Evenframe)]
            #[indexes(author_name(fields("written_by.display_name"), unique))]
            pub struct Post {
                pub id: String,
                #[surreal(rename = "author")]
                pub written_by: Author,
            }
        "#,
    )
    .expect("a nested path into a renamed struct must be accepted");
    let statements: Vec<String> = tables["post"]
        .all_indexes("post", &objects)
        .iter()
        .map(|index| index.define_statement("post"))
        .collect();
    assert_eq!(
        statements,
        vec!["DEFINE INDEX OVERWRITE author_name ON TABLE post FIELDS author.displayName UNIQUE;"]
    );
}
