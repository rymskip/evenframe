//! A build script's schema dump: `build::schemadump()` run the way Cargo
//! runs a build script, from a project's manifest directory, scanning its
//! sources and writing the schema without a database.
#![cfg(feature = "build-schemadump")]

use evenframe_core::build::schemadump;
use std::fs;
use tempfile::TempDir;

const SOURCE: &str = r#"
use evenframe::Evenframe;

#[derive(Evenframe)]
pub struct Author {
    pub id: String,
    #[validators(StringValidator::NonEmpty)]
    pub name: String,
    pub cooldown: std::time::Duration,
}
"#;

const CONFIG: &str = r#"
[schemasync]
should_generate_mocks = false

[schemasync.database]
provider = "surrealdb"
url = "${SURREALDB_URL}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"
timeout = 60
"#;

#[test]
fn a_build_script_dumps_the_schema_without_a_database() {
    let project = TempDir::new().unwrap();
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"dumped\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/lib.rs"), SOURCE).unwrap();
    fs::write(project.path().join("evenframe.toml"), CONFIG).unwrap();

    let dump = || {
        temp_env::with_vars(
            [
                ("CARGO_MANIFEST_DIR", Some(project.path().as_os_str())),
                ("SURREALDB_URL", None),
            ],
            schemadump,
        )
        .unwrap()
    };

    let path = dump();
    assert_eq!(path, project.path().join(".evenframe/surql/schema.surql"));
    let schema = fs::read_to_string(&path).unwrap();
    assert!(schema.contains("DEFINE TABLE OVERWRITE author"), "{schema}");
    assert!(
        schema.contains("DEFINE FIELD OVERWRITE cooldown ON TABLE author TYPE duration"),
        "{schema}"
    );

    let written = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(dump(), path);
    assert_eq!(
        fs::metadata(&path).unwrap().modified().unwrap(),
        written,
        "an unchanged schema leaves the file untouched"
    );
}
