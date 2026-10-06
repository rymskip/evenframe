//! The evenframe CLI built from this checkout.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The evenframe binary built from this checkout, once per test run.
pub fn evenframe() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let status = Command::new("cargo")
            .args(["build", "-p", "evenframe"])
            .current_dir(&workspace)
            .status()
            .expect("cargo build should run");
        assert!(status.success(), "building evenframe failed");
        workspace.join("target/debug/evenframe")
    })
}
