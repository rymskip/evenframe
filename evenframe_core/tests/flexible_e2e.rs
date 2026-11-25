//! A schemafull table keeps the keys of an object it does not list only on a
//! FLEXIBLE field, and SurrealDB 4 takes FLEXIBLE only after the TYPE. Each case
//! applies the scanned project's schema to SurrealDB and writes keys through it.

#![cfg(all(feature = "schemasync", feature = "scan"))]

use evenframe_core::scan::{ScanConfig, build_all_configs};
use evenframe_core::schemasync::dump::tables_surql;
use evenframe_core::types::ForeignTypeRegistry;
use std::fs;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use tempfile::TempDir;

const SOURCE: &str = r#"
    use evenframe::Evenframe;
    use std::collections::BTreeMap;

    #[derive(Evenframe)]
    pub struct Extras {
        pub label: String,
        #[serde(flatten)]
        pub rest: BTreeMap<String, String>,
    }

    #[derive(Evenframe)]
    pub struct Payload {
        pub id: String,
        #[define_field_statement(data_type("object"), default("{}"))]
        pub body: String,
        #[define_field_statement(data_type("array<{ data: object }>"), default("[]"))]
        pub items: String,
        pub extras: Extras,
        pub counts: BTreeMap<String, u32>,
    }
"#;

fn schema() -> String {
    let project = TempDir::new().unwrap();
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"flexible_fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir_all(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/lib.rs"), SOURCE).unwrap();
    let types = build_all_configs(&ScanConfig {
        scan_path: project.path().to_path_buf(),
        ..ScanConfig::default()
    })
    .expect("the project scans")
    .into_schemasync()
    .expect("the schemasync view");
    tables_surql(
        &types,
        &ForeignTypeRegistry::default(),
        evenframe_core::schemasync::config::SurqlOptions::default(),
    )
    .expect("the schema generates")
}

async fn database(schema: &str) -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();
    db.query(schema)
        .await
        .unwrap_or_else(|error| panic!("the schema runs: {error}\n{schema}"))
        .check()
        .unwrap_or_else(|error| panic!("every DEFINE succeeds: {error}\n{schema}"));
    db
}

/// The DEFINE FIELD statement of `field` on `payload`.
fn field_statement<'schema>(schema: &'schema str, field: &str) -> &'schema str {
    let prefix = format!("DEFINE FIELD OVERWRITE {field} ON TABLE payload ");
    schema
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no statement for {field}:\n{schema}"))
}

#[test]
fn an_unlisted_object_is_flexible_after_its_type_and_a_typed_map_is_not() {
    let schema = schema();
    for field in ["body", "items", "extras"] {
        let statement = field_statement(&schema, field);
        let flexible = statement
            .find(" FLEXIBLE")
            .unwrap_or_else(|| panic!("{field} keeps no keys without FLEXIBLE: {statement}"));
        let typed = statement
            .find(" TYPE ")
            .unwrap_or_else(|| panic!("{field} has no TYPE: {statement}"));
        assert!(typed < flexible, "FLEXIBLE precedes TYPE: {statement}");
    }
    let counts = field_statement(&schema, "counts");
    assert!(
        !counts.contains("FLEXIBLE"),
        "a map whose values `.*` types stays strict: {counts}"
    );
}

#[tokio::test]
async fn keys_the_schema_does_not_list_are_stored() {
    let db = database(&schema()).await;
    let mut response = db
        .query(
            "CREATE payload:one CONTENT { \
                 body: { a: { b: [1, 2] } }, \
                 items: [{ data: { k: 'v', n: { m: 1 } } }], \
                 extras: { label: 'x', other: 'y' }, \
                 counts: { seen: 3 } \
             }; \
             SELECT VALUE { body: body.a.b, item: items[0].data.n.m, extra: extras.other } \
                 FROM ONLY payload:one;",
        )
        .await
        .expect("the query is sent")
        .check()
        .unwrap_or_else(|error| panic!("the nested keys are refused: {error}"));
    let stored: Option<serde_json::Value> = response.take(1).unwrap();
    assert_eq!(
        stored,
        Some(serde_json::json!({ "body": [1, 2], "item": 1, "extra": "y" }))
    );
}
