//! Runs the `evenframe` binary against a scratch copy of a fixture project
//! and snapshots what a user sees: the exit status, stdout and stderr.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch copy of the fixture project, since commands write into it.
fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    copy_dir(&fixture("project"), dir.path());
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Runs `evenframe <args>` in `dir`, rendered as a transcript step. Cargo's
/// and the caller's environment are cleared so config discovery starts from
/// `dir` alone, as it does for a user in their shell.
fn run(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_evenframe"))
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .output()
        .unwrap();
    format!(
        "$ evenframe {}\nstatus: {:?}\n--- stdout ---\n{}--- stderr ---\n{}\n",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

/// Every file under `dir/sub`, relative to `dir`, as a transcript step.
fn files_under(dir: &Path, sub: &str) -> String {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().display().to_string());
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &dir.join(sub), &mut files);
    files.sort();
    format!("$ ls -R {sub}\n{}\n\n", files.join("\n"))
}

/// Snapshots `transcript` with the scratch directory and log timestamps
/// masked, so the snapshot is stable across machines and runs.
fn assert_transcript(dir: &Path, name: &str, transcript: &str) {
    let mut settings = insta::Settings::clone_current();
    // The canonical path first: on macOS it extends the raw temp path.
    settings.add_filter(
        &regex::escape(&dir.canonicalize().unwrap().to_string_lossy()),
        "[PROJECT]",
    );
    settings.add_filter(&regex::escape(&dir.to_string_lossy()), "[PROJECT]");
    settings.add_filter(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z", "[TIME]");
    settings.bind(|| insta::assert_snapshot!(name, transcript));
}

#[test]
fn check_follows_the_sources() {
    let dir = project();
    let root = dir.path();
    let mut transcript = run(root, &["check"]);
    transcript += &run(root, &["validate", "--types-only"]);
    transcript += &run(root, &["check"]);
    let lib = root.join("src/lib.rs");
    fs::write(&lib, fs::read_to_string(&lib).unwrap() + "\n// edited\n").unwrap();
    transcript += &run(root, &["check"]);
    transcript += &run(root, &["check", "--json"]);
    assert_transcript(root, "check_follows_the_sources", &transcript);
}

#[test]
fn validate_reports_each_check() {
    let dir = project();
    let root = dir.path();
    let mut transcript = run(root, &["validate"]);
    fs::copy(
        fixture("invalid-typesync.toml"),
        root.join("evenframe.toml"),
    )
    .unwrap();
    transcript += &run(root, &["validate"]);
    assert_transcript(root, "validate_reports_each_check", &transcript);
}

#[test]
fn init_writes_a_config_the_other_commands_accept() {
    let dir = project();
    let root = dir.path();
    fs::remove_file(root.join("evenframe.toml")).unwrap();
    let mut transcript = run(root, &["init", "--minimal"]);
    transcript += &run(root, &["validate"]);
    transcript += &run(root, &["init"]);
    transcript += &run(root, &["init", "--force"]);
    transcript += &run(root, &["validate"]);
    assert_transcript(
        root,
        "init_writes_a_config_the_other_commands_accept",
        &transcript,
    );
}

#[test]
fn config_flag_selects_the_project() {
    let project = project();
    let elsewhere = TempDir::new().unwrap();
    let config = project.path().join("evenframe.toml");
    let config = config.to_str().unwrap();
    let mut transcript = run(elsewhere.path(), &["check", "--config", config]);
    transcript += &run(
        elsewhere.path(),
        &["validate", "--types-only", "--config", config],
    );
    transcript += &run(elsewhere.path(), &["check", "--config", config]);
    transcript += &run(elsewhere.path(), &["check"]);
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(
        &regex::escape(&elsewhere.path().canonicalize().unwrap().to_string_lossy()),
        "[ELSEWHERE]",
    );
    settings.add_filter(
        &regex::escape(&elsewhere.path().to_string_lossy()),
        "[ELSEWHERE]",
    );
    settings.bind(|| {
        assert_transcript(
            project.path(),
            "config_flag_selects_the_project",
            &transcript,
        )
    });
}

#[test]
fn init_writes_to_the_config_flag_path() {
    let dir = project();
    let root = dir.path();
    fs::remove_file(root.join("evenframe.toml")).unwrap();
    let mut transcript = run(
        root,
        &["init", "--minimal", "--config", ".evenframe/config.toml"],
    );
    transcript += &run(root, &["validate"]);
    assert_transcript(root, "init_writes_to_the_config_flag_path", &transcript);
}

#[test]
fn typesync_writes_under_the_project_root() {
    let dir = project();
    let root = dir.path();
    let mut transcript = run(root, &["typesync"]);
    transcript += &files_under(root, "generated");
    fs::remove_dir_all(root.join("generated")).unwrap();
    transcript += &run(&root.join("src"), &["typesync"]);
    transcript += &files_under(root, "generated");
    transcript += &run(root, &["typesync", "--output", "out"]);
    transcript += &files_under(root, "out");
    transcript += &run(root, &["typesync", "arktype", "-o", "single/types.ts"]);
    transcript += &files_under(root, "single");
    transcript += &run(root, &["typesync", "effect", "-o", "effect.ts"]);
    transcript += &run(root, &["--output", "fx", "typesync", "effect"]);
    transcript += &files_under(root, "fx");
    assert_transcript(root, "typesync_writes_under_the_project_root", &transcript);
}

#[test]
fn info_reports_each_section_in_every_format() {
    let dir = project();
    let root = dir.path();
    let mut transcript = run(root, &["info"]);
    transcript += &run(root, &["info", "--settings", "--format", "json"]);
    transcript += &run(root, &["info", "--types", "--format", "yaml"]);
    assert_transcript(
        root,
        "info_reports_each_section_in_every_format",
        &transcript,
    );
}
