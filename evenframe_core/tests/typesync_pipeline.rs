//! Every TypeScript output of a small scanned project, through the same
//! filtering, merging and rendering the CLI runs, so a change to how outputs
//! look up, order or group types shows up as a snapshot diff.
#![cfg(all(
    feature = "macroforge",
    feature = "schemasync",
    feature = "build-typesync"
))]

use evenframe_core::config::ForeignTypeConfig;
use evenframe_core::scan::{
    ScanConfig, build_all_configs, filter_for_typesync, merge_tables_and_objects,
};
use evenframe_core::types::ForeignTypeRegistry;
use evenframe_core::typesync::config::{OutputKind, OutputMode, TypesyncOutput};
use evenframe_core::typesync::output::{OutputTypes, render_output};
use std::collections::BTreeMap;
use std::fs;
use tempfile::TempDir;

const SOURCE: &str = r#"
use evenframe::Evenframe;
use evenframe::types::RecordLink;

#[derive(Evenframe)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub home: Address,
}

#[derive(Evenframe)]
pub struct Post {
    pub id: String,
    pub title: String,
    pub author: RecordLink<Author>,
    pub read_time: std::time::Duration,
    pub status: Status,
    pub location: Address,
    pub outline: Option<Section>,
    pub comments: Vec<Comment>,
}

/// Shared by two types, so it gets its own file.
#[derive(Evenframe)]
pub struct Address {
    pub street: String,
    pub city: Option<String>,
}

#[derive(Evenframe)]
pub enum Status {
    Draft,
    Scheduled { at: String, note: Option<String> },
    Archived(String),
}

/// Recursive through itself.
#[derive(Evenframe)]
pub struct Section {
    pub heading: String,
    pub children: Vec<Section>,
}

/// Recursive through `Reply`.
#[derive(Evenframe)]
pub struct Comment {
    pub body: String,
    pub replies: Vec<Reply>,
}

#[derive(Evenframe)]
pub struct Reply {
    pub body: String,
    pub thread: Vec<Comment>,
}
"#;

fn outputs() -> Vec<TypesyncOutput> {
    let per_file = |kind| {
        let mut output = TypesyncOutput::new(kind, "unused");
        output.files.mode = OutputMode::PerFile;
        output
    };
    vec![
        TypesyncOutput::new(OutputKind::Arktype, "unused"),
        TypesyncOutput::new(OutputKind::Effect, "unused"),
        per_file(OutputKind::Effect),
        TypesyncOutput::new(OutputKind::Macroforge, "unused"),
        per_file(OutputKind::Macroforge),
    ]
}

/// The playground's mapping of the SDK's record id, whose codec the generated
/// check places beside these outputs at `../record-id.ts`.
fn playground_record_id() -> ForeignTypeConfig {
    let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../evenframe_playground/evenframe.toml");
    let config: toml::Table = fs::read_to_string(&config_path).unwrap().parse().unwrap();
    config["general"]["foreign_types"]["RecordId"]
        .clone()
        .try_into()
        .unwrap()
}

#[test]
fn every_output_of_a_scanned_project() {
    let project = TempDir::new().unwrap();
    fs::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"pipeline\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/lib.rs"), SOURCE).unwrap();
    let config = ScanConfig {
        scan_path: project.path().to_path_buf(),
        ..ScanConfig::default()
    };

    let (enums, tables, objects) = build_all_configs(&config).unwrap();
    let (enums, tables, objects) = filter_for_typesync(&enums, &tables, &objects);
    let structs = merge_tables_and_objects(tables, objects);
    let registry = ForeignTypeRegistry::from_config(&BTreeMap::from([(
        "RecordId".to_string(),
        playground_record_id(),
    )]));
    let types = OutputTypes::new(&structs, &enums, &registry).unwrap();

    let out = TempDir::new().unwrap();
    let mut rendered = String::new();
    for (index, output) in outputs().iter().enumerate() {
        let dir = out.path().join(format!("{index}-{}", output.kind));
        let mut files = render_output(output, &dir, None, &types)
            .unwrap()
            .write()
            .unwrap();
        files.sort_by(|left, right| left.path.cmp(&right.path));
        for file in files {
            let relative = file.path.strip_prefix(out.path()).unwrap();
            rendered.push_str(&format!("=== {}\n", relative.display()));
            rendered.push_str(&fs::read_to_string(&file.path).unwrap());
            rendered.push('\n');
        }
    }
    insta::assert_snapshot!(rendered);
}
