//! The testground's test plugins, built for WASM from their source, so a
//! test never loads a stale or missing build.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, PoisonError};

/// Builds the test plugin `name` under `test_plugins/`, once per test run,
/// and returns its `.wasm`.
pub fn build_plugin(name: &str) -> PathBuf {
    static BUILT: Mutex<BTreeMap<String, PathBuf>> = Mutex::new(BTreeMap::new());
    // A build that failed in another test poisons the lock; this test then
    // builds again and reports that failure itself.
    let mut built = BUILT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(path) = built.get(name) {
        return path.clone();
    }
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_plugins")
        .join(name);
    let status = Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(plugin_root.join("Cargo.toml"))
        .args(["--target", "wasm32-unknown-unknown", "--release"])
        .env("RUSTFLAGS", "-D warnings")
        .status()
        .unwrap_or_else(|error| panic!("cargo should run to build the {name} plugin: {error}"));
    assert!(
        status.success(),
        "building the {name} plugin failed. It needs the wasm32-unknown-unknown target: \
         `rustup target add wasm32-unknown-unknown`"
    );
    let wasm = plugin_root.join(format!("target/wasm32-unknown-unknown/release/{name}.wasm"));
    assert!(
        wasm.exists(),
        "the {name} plugin built, but {} is missing",
        wasm.display()
    );
    built.insert(name.to_owned(), wasm.clone());
    wasm
}
