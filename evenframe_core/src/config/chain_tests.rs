use super::super::EvenframeConfig;
use super::{chain_ending_at, find_config_chain_from};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Writes `contents` to `<dir>/.evenframe/config.toml` and returns its path.
fn write_config(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join(".evenframe").join("config.toml");
    fs::create_dir_all(path.parent().expect("a config path has a parent")).expect("mkdir");
    fs::write(&path, contents).expect("write config");
    path
}

const ROOT: &str = r#"
[general]
apply_aliases = ["root_alias"]

[general.foreign_types.Decimal]
rust_type_names = ["Decimal"]
surrealdb = "decimal"
macroforge = { type = "string" }

[typesync]
outputs = [{ kind = "macroforge", dir = "generated" }]
"#;

#[test]
fn the_chain_runs_from_the_outermost_config_to_the_nearest() {
    let temp = TempDir::new().expect("tempdir");
    let root = write_config(temp.path(), ROOT);
    let project_dir = temp.path().join("core").join("backend");
    let project = write_config(&project_dir, "");

    assert_eq!(
        find_config_chain_from(&project_dir.join("src")),
        [root.clone(), project.clone()]
    );
    assert_eq!(chain_ending_at(&project), [root, project]);
}

#[test]
fn a_bare_project_config_defers_to_the_one_above_it() {
    let temp = TempDir::new().expect("tempdir");
    write_config(temp.path(), ROOT);
    let project = write_config(&temp.path().join("backend"), "");

    let config = EvenframeConfig::load_from(project.clone(), false).expect("loads");

    assert_eq!(config.config_file_path, project);
    assert_eq!(config.general.apply_aliases, ["root_alias"]);
    assert!(config.general.foreign_types.contains_key("Decimal"));
    let [output] = config.typesync.outputs.as_slice() else {
        panic!("one output inherited, got {:?}", config.typesync.outputs);
    };
    // Relative to the config that wrote it, not the project inheriting it.
    assert_eq!(Path::new(&output.dir), temp.path().join("generated"));
}

#[test]
fn the_nearer_config_wins_tables_merge_and_arrays_replace() {
    let temp = TempDir::new().expect("tempdir");
    write_config(temp.path(), ROOT);
    let project = write_config(
        &temp.path().join("backend"),
        r#"
[general]
apply_aliases = ["project_alias"]

[general.foreign_types.Decimal]
surrealdb = "number"
"#,
    );

    let config = EvenframeConfig::load_from(project, false).expect("loads");

    assert_eq!(config.general.apply_aliases, ["project_alias"]);
    let decimal = &config.general.foreign_types["Decimal"];
    assert_eq!(decimal.surrealdb, "number");
    assert_eq!(decimal.rust_type_names, ["Decimal"], "kept from the root");
}

#[test]
fn a_project_config_replaces_the_outputs_in_either_spelling() {
    let temp = TempDir::new().expect("tempdir");
    write_config(temp.path(), ROOT);
    let project = write_config(
        &temp.path().join("backend"),
        r#"
[typesync]
output = { kind = "effect", dir = "effect" }
"#,
    );

    let config = EvenframeConfig::load_from(project, false).expect("loads");

    let [output] = config.typesync.outputs.as_slice() else {
        panic!(
            "the project's one output, got {:?}",
            config.typesync.outputs
        );
    };
    assert_eq!(
        Path::new(&output.dir),
        temp.path().join("backend").join("effect")
    );
}

#[test]
fn every_relative_path_is_relative_to_its_own_config() {
    let temp = TempDir::new().expect("tempdir");
    write_config(
        temp.path(),
        r#"
[general]
include_files = ["shared/types.rs", { path = "shared/only.rs", resolve_only = true }]
output_rule_plugins = { rules = { path = "plugins/rules.wasm" } }
"#,
    );
    let project_dir = temp.path().join("backend");
    let project = write_config(
        &project_dir,
        r#"
[general]
synthetic_item_plugins = { routes = { path = "plugins/routes.wasm" } }
"#,
    );

    let config = EvenframeConfig::load_from(project, false).expect("loads");

    let includes: Vec<PathBuf> = config
        .resolved_include_files()
        .into_iter()
        .map(|include| include.path)
        .collect();
    assert_eq!(
        includes,
        [
            temp.path().join("shared/types.rs"),
            temp.path().join("shared/only.rs")
        ]
    );
    assert_eq!(
        Path::new(&config.general.output_rule_plugins["rules"].path),
        temp.path().join("plugins/rules.wasm")
    );
    assert_eq!(
        Path::new(&config.general.synthetic_item_plugins["routes"].path),
        project_dir.join("plugins/routes.wasm")
    );
}

#[test]
fn each_config_loads_its_own_env_and_the_nearer_value_wins() {
    let temp = TempDir::new().expect("tempdir");
    // Names no other test sets, since the environment is process-wide.
    fs::write(
        temp.path().join(".env"),
        "EVENFRAME_CHAIN_SHARED=root\nEVENFRAME_CHAIN_ROOT_ONLY=root\n",
    )
    .expect("write env");
    write_config(
        temp.path(),
        r#"
[general]
apply_aliases = ["${EVENFRAME_CHAIN_SHARED}", "${EVENFRAME_CHAIN_ROOT_ONLY}"]
"#,
    );
    let project_dir = temp.path().join("backend");
    fs::create_dir_all(&project_dir).expect("mkdir");
    fs::write(project_dir.join(".env"), "EVENFRAME_CHAIN_SHARED=project\n").expect("write env");
    let project = write_config(&project_dir, "");

    let config = EvenframeConfig::load_from(project, false).expect("loads");

    assert_eq!(config.general.apply_aliases, ["project", "root"]);
}

#[test]
fn projects_belong_to_the_config_that_declares_them() {
    let temp = TempDir::new().expect("tempdir");
    let root = write_config(
        temp.path(),
        r#"
[general]
projects = ["backend"]
"#,
    );
    let project = write_config(&temp.path().join("backend"), "");

    let root_config = EvenframeConfig::load_from(root, false).expect("root loads");
    let project_config = EvenframeConfig::load_from(project, false).expect("project loads");

    assert_eq!(root_config.general.projects, ["backend"]);
    assert!(project_config.general.projects.is_empty());
}

#[test]
fn a_bad_setting_anywhere_in_the_chain_names_the_files() {
    let temp = TempDir::new().expect("tempdir");
    let root = write_config(temp.path(), "[general]\napply_aliases = 3\n");
    let project = write_config(&temp.path().join("backend"), "");

    let error = EvenframeConfig::load_from(project.clone(), false)
        .expect_err("a number is not a list of aliases")
        .to_string();

    assert!(error.contains(&root.display().to_string()), "{error}");
    assert!(error.contains(&project.display().to_string()), "{error}");
}

#[test]
fn foreign_types_merge_over_the_chain_without_the_environment() {
    let temp = TempDir::new().expect("tempdir");
    write_config(
        temp.path(),
        r#"
[general.foreign_types.Decimal]
rust_type_names = ["Decimal"]
surrealdb = "decimal"

[schemasync.database]
url = "${EVENFRAME_CHAIN_NEVER_SET}"
"#,
    );
    let project_dir = temp.path().join("backend");
    write_config(
        &project_dir,
        r#"
[general.foreign_types.RecordId]
surrealdb = "record<any>"
"#,
    );

    let foreign_types =
        EvenframeConfig::foreign_types_from(&project_dir.join("src")).expect("reads");

    assert_eq!(
        foreign_types.keys().collect::<Vec<_>>(),
        ["Decimal", "RecordId"]
    );
}
