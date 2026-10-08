use super::{Focus, Workspace};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_config(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join(".evenframe").join("config.toml");
    fs::create_dir_all(path.parent().expect("a config path has a parent")).expect("mkdir");
    fs::write(&path, contents).expect("write config");
    path
}

const ROOT: &str = r#"
[general]
projects = ["core/backend", "idp/backend"]

[typesync]
output = { kind = "macroforge", dir = "types" }
"#;

struct Layout {
    _temp: TempDir,
    root: PathBuf,
    core: PathBuf,
    idp: PathBuf,
}

fn layout() -> Layout {
    let temp = TempDir::new().expect("tempdir");
    let root = write_config(temp.path(), ROOT);
    let core = write_config(&temp.path().join("core/backend"), "");
    let idp = write_config(
        &temp.path().join("idp/backend"),
        "[general]\napply_aliases = [\"idp\"]\n",
    );
    Layout {
        _temp: temp,
        root,
        core,
        idp,
    }
}

#[test]
fn a_chain_without_projects_is_no_workspace() {
    let temp = TempDir::new().expect("tempdir");
    let only = write_config(temp.path(), "");

    assert!(
        Workspace::from_nearest(&only, false)
            .expect("loads")
            .is_none()
    );
}

#[test]
fn the_workspace_root_focuses_on_every_project() {
    let layout = layout();

    let workspace = Workspace::from_nearest(&layout.root, false)
        .expect("loads")
        .expect("a workspace");

    assert_eq!(workspace.focus, Focus::All);
    let names: Vec<&str> = workspace
        .focused()
        .map(|project| project.name.as_str())
        .collect();
    assert_eq!(names, ["core/backend", "idp/backend"]);
    assert_eq!(
        workspace.root(),
        layout.root.parent().and_then(Path::parent).expect("root")
    );
}

#[test]
fn a_project_directory_focuses_on_that_project() {
    let layout = layout();

    let workspace = Workspace::from_nearest(&layout.idp, false)
        .expect("loads")
        .expect("a workspace");

    assert_eq!(workspace.focus, Focus::Project("idp/backend".to_string()));
    let [focused] = workspace.focused().collect::<Vec<_>>()[..] else {
        panic!("one focused project");
    };
    assert_eq!(focused.config.general.apply_aliases, ["idp"]);
    assert_eq!(focused.config.config_file_path, layout.idp);
    // Every project inherits the workspace's typesync settings.
    assert_eq!(workspace.projects[0].config.config_file_path, layout.core);
    assert_eq!(workspace.projects[0].config.typesync.outputs.len(), 1);
}

#[test]
fn a_project_without_a_config_is_an_error() {
    let temp = TempDir::new().expect("tempdir");
    let root = write_config(temp.path(), "[general]\nprojects = [\"missing\"]\n");

    let error = Workspace::from_nearest(&root, false)
        .expect_err("the project has no config")
        .to_string();

    assert!(error.contains("names the project `missing`"), "{error}");
}

#[test]
fn a_workspace_inside_a_workspace_is_an_error() {
    let temp = TempDir::new().expect("tempdir");
    let root = write_config(temp.path(), "[general]\nprojects = [\"inner\"]\n");
    write_config(
        &temp.path().join("inner"),
        "[general]\nprojects = [\"deeper\"]\n",
    );
    write_config(&temp.path().join("inner/deeper"), "");

    let error = Workspace::from_nearest(&root, false)
        .expect_err("nested workspaces")
        .to_string();

    assert!(
        error.contains("a workspace cannot contain another"),
        "{error}"
    );
}

#[test]
fn a_config_inside_the_workspace_that_is_no_project_is_an_error() {
    let layout = layout();
    let stray = write_config(
        &layout
            .root
            .parent()
            .and_then(Path::parent)
            .expect("root")
            .join("tools"),
        "",
    );

    let error = Workspace::from_nearest(&stray, false)
        .expect_err("not a project")
        .to_string();

    assert!(error.contains("is not one of its projects"), "{error}");
}

#[test]
fn a_config_file_in_a_projects_directory_stands_in_for_its_own() {
    let layout = layout();
    let stand_in = layout
        .idp
        .parent()
        .expect("a config path has a parent")
        .join("scoped.toml");
    fs::write(&stand_in, "[general]\napply_aliases = [\"scoped\"]\n").expect("write config");

    let workspace = Workspace::from_nearest(&stand_in, false)
        .expect("loads")
        .expect("a workspace");

    assert_eq!(workspace.focus, Focus::Project("idp/backend".to_owned()));
    let idp = workspace
        .projects
        .iter()
        .find(|project| project.name == "idp/backend")
        .expect("the idp project");
    assert_eq!(idp.config.config_file_path, stand_in);
    assert_eq!(idp.config.general.apply_aliases, vec!["scoped".to_owned()]);
}

#[test]
fn naming_one_project_twice_is_an_error() {
    let temp = TempDir::new().expect("tempdir");
    let root = write_config(
        temp.path(),
        "[general]\nprojects = [\"backend\", \"./backend\"]\n",
    );
    write_config(&temp.path().join("backend"), "");

    let error = Workspace::from_nearest(&root, false)
        .expect_err("named twice")
        .to_string();

    assert!(error.contains("twice"), "{error}");
}
