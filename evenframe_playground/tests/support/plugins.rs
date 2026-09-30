//! The playground's test plugins, built for WASM.

use std::path::PathBuf;
use std::process::Command;

/// Builds the test plugin `name` under `test_plugins/` and returns its `.wasm`.
pub fn build_plugin(name: &str) -> PathBuf {
    let plugin_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_plugins")
        .join(name);
    let status = Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(plugin_root.join("Cargo.toml"))
        .args(["--target", "wasm32-unknown-unknown", "--release"])
        .status()
        .unwrap_or_else(|error| panic!("cargo should run to build the {name} plugin: {error}"));
    assert!(
        status.success(),
        "building the {name} plugin failed. It needs the wasm32-unknown-unknown target: \
         `rustup target add wasm32-unknown-unknown`"
    );
    let built = plugin_root.join(format!("target/wasm32-unknown-unknown/release/{name}.wasm"));
    assert!(
        built.exists(),
        "the {name} plugin built, but {} is missing",
        built.display()
    );
    built
}
