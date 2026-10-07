//! The schema asserts a validator wherever the stored value holds it: inside
//! an embedded object, an array, a map and a tagged enum's payload. Each case
//! applies the scanned project's schema to SurrealDB and writes through it.

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
    #[validators(StringValidator::NonEmpty)]
    pub struct StepId(String);

    #[derive(Evenframe)]
    pub struct Waiting { pub step: StepId, pub parents: Vec<StepId> }

    #[derive(Evenframe)]
    pub enum Change { Renamed { to: StepId }, Idle }

    #[derive(Evenframe)]
    #[serde(untagged)]
    pub enum Party {
        Company { company_name: StepId },
        Person { first_name: StepId, last_name: StepId },
    }

    #[derive(Evenframe)]
    pub struct Run {
        pub id: String,
        pub waiting: Vec<Waiting>,
        pub labels: BTreeMap<StepId, u32>,
        pub change: Change,
        pub party: Party,
    }
"#;

async fn database() -> Surreal<Db> {
    let project = TempDir::new().unwrap();
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"nested_assert_fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
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
    let schema =
        tables_surql(&types, &ForeignTypeRegistry::default(), false).expect("the schema generates");
    let db = Surreal::new::<Mem>(()).await.unwrap();
    db.use_ns("test").use_db("test").await.unwrap();
    db.query(schema.as_str())
        .await
        .unwrap_or_else(|error| panic!("the schema runs: {error}\n{schema}"))
        .check()
        .unwrap_or_else(|error| panic!("every DEFINE succeeds: {error}\n{schema}"));
    db
}

/// Whether SurrealDB stores `content` as a `run` record.
async fn stores(db: &Surreal<Db>, content: &str) -> bool {
    db.query(format!("CREATE run CONTENT {content}"))
        .await
        .expect("the query is sent")
        .check()
        .is_ok()
}

/// A `run` record's content, with each field's value as `fields` gives it
/// and a valid one for the rest.
fn run(fields: &[(&str, &str)]) -> String {
    let valid = [
        ("waiting", "[{ step: 'a', parents: ['b'] }]"),
        ("labels", "{ x: 1 }"),
        ("change", "{ Renamed: { to: 'c' } }"),
        ("party", "{ company_name: 'd' }"),
    ];
    let entries: Vec<String> = valid
        .iter()
        .map(|(name, value)| {
            let value = fields
                .iter()
                .find(|(field, _)| field == name)
                .map_or(*value, |(_, given)| *given);
            format!("{name}: {value}")
        })
        .collect();
    format!("{{ {} }}", entries.join(", "))
}

#[tokio::test]
async fn a_value_meeting_every_nested_validator_is_stored() {
    let db = database().await;
    for content in [
        run(&[]),
        run(&[
            ("waiting", "[]"),
            ("labels", "{}"),
            ("change", "'Idle'"),
            ("party", "{ first_name: 'a', last_name: 'b' }"),
        ]),
    ] {
        assert!(stores(&db, &content).await, "{content} is stored");
    }
}

#[tokio::test]
async fn a_value_failing_a_nested_validator_is_refused() {
    let db = database().await;
    for content in [
        // An element of an array inside an object inside an array.
        run(&[("waiting", "[{ step: 'a', parents: [''] }]")]),
        // An embedded object's member.
        run(&[("waiting", "[{ step: '', parents: [] }]")]),
        // A map's key.
        run(&[("labels", "{ '': 1 }")]),
        // A tagged variant's payload.
        run(&[("change", "{ Renamed: { to: '' } }")]),
        // Each variant of an untagged enum.
        run(&[("party", "{ company_name: '' }")]),
        run(&[("party", "{ first_name: 'a', last_name: '' }")]),
    ] {
        assert!(!stores(&db, &content).await, "{content} is refused");
    }
}
