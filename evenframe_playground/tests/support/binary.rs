//! The evenframe CLI built from this checkout, and copies of the playground
//! to run it on.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn playground() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The evenframe binary built from this checkout, once per test run.
pub fn evenframe() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let workspace = playground().join("..");
        let status = Command::new("cargo")
            .args(["build", "-p", "evenframe"])
            .current_dir(&workspace)
            .status()
            .expect("cargo build should run");
        assert!(status.success(), "building evenframe failed");
        workspace.join("target/debug/evenframe")
    })
}

/// Copies the playground's config, models and surql files into `to`.
pub fn copy_playground(to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in ["Cargo.toml", "evenframe.toml", "src", "surql"] {
        copy(&playground().join(entry), &to.join(entry));
    }
}

fn copy(from: &Path, to: &Path) {
    if from.is_dir() {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            copy(&entry.path(), &to.join(entry.file_name()));
        }
    } else {
        fs::copy(from, to).unwrap();
    }
}
