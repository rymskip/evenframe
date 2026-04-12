//! End-to-end tests for the `expand_macros` workspace scanner path.
//!
//! These tests exercise the fixes from the "expander 0-byte files" bug:
//!
//! - `workspace_scanner.rs` no longer swallows missing modules from the
//!   split expanded output; it errors out loudly.
//! - `expansion_cache::write_fragment` refuses to write empty fragments.
//! - `CacheManifest::load` discards a cache whose fragments are missing or
//!   empty on disk (automatic recovery from poisoned caches).
//! - `walk_src` skips `src/main.rs` and `src/bin/*` so bin targets don't
//!   look like "missing module" errors during `cargo expand --lib`.
//! - Under `expand_macros = true`, manifests are processed sequentially so
//!   parallel `cargo expand` invocations don't contend on the build lock.
//!
//! Each test creates a *fresh* temp crate with its own `target/` dir; we
//! deliberately do not share the playground's `target/` to avoid polluting
//! it with cargo-expand artifacts.
//!
//! Two of the tests shell out to `cargo expand`. If cargo-expand is not
//! installed, those tests are skipped with a warning instead of failing —
//! the CI host needs `cargo install cargo-expand` to exercise them fully.
//!
//! Run with: `cargo test --test expander_e2e_test`

use evenframe_core::tooling::WorkspaceScanner;
use evenframe_core::tooling::expansion_cache::{self, CacheEntry, CacheManifest};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

// ============================================================================
// Helpers
// ============================================================================

fn cargo_expand_available() -> bool {
    Command::new("cargo")
        .args(["expand", "--help"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Write a minimal standalone lib crate that defines a handful of types
/// detected by the scanner via manual trait impls.
///
/// We use manual impls instead of `#[derive(Evenframe)]` so the temp crate
/// doesn't need to depend on `evenframe_derive` — that lets `cargo expand`
/// run against it without pulling in the whole workspace.
fn write_minimal_lib_crate(dir: &Path, crate_name: &str) {
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{crate_name}"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[dependencies]
"#
        ),
    )
    .unwrap();

    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("lib.rs"),
        r#"//! Stub crate for expander e2e tests.

pub trait EvenframePersistableStruct {}
pub trait EvenframeAppStruct {}
pub trait EvenframeTaggedUnion {}

pub struct User {
    pub id: String,
    pub name: String,
}
impl EvenframePersistableStruct for User {}

pub struct Profile {
    pub bio: String,
}
impl EvenframeAppStruct for Profile {}

pub mod sub_one;
pub mod sub_two;
"#,
    )
    .unwrap();

    fs::write(
        src.join("sub_one.rs"),
        r#"use crate::EvenframeAppStruct;

pub struct Widget {
    pub kind: String,
}
impl EvenframeAppStruct for Widget {}
"#,
    )
    .unwrap();

    fs::write(
        src.join("sub_two.rs"),
        r#"use crate::EvenframeTaggedUnion;

pub enum Color {
    Red,
    Green,
    Blue,
}
impl EvenframeTaggedUnion for Color {}
"#,
    )
    .unwrap();
}

/// Walk a directory tree and return every file path with its byte length.
fn collect_file_sizes(dir: &Path) -> Vec<(PathBuf, u64)> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                let md = fs::metadata(&path).unwrap();
                out.push((path, md.len()));
            }
        }
    }
    walk(dir, &mut out);
    out
}

// ============================================================================
// Tests that do NOT need cargo expand
// ============================================================================

#[test]
fn load_cache_discards_manifest_pointing_at_missing_fragment() {
    // This exercises the CacheManifest::load validator (fix 1c):
    // if the manifest references a fragment that doesn't exist on disk,
    // the entire cache must be thrown away so the next run re-expands.
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path();

    let mut m = CacheManifest::empty("test_crate");
    m.entries.insert(
        "lib.rs".to_string(),
        CacheEntry {
            input_hash: "deadbeef".to_string(),
            module_path: "test_crate".to_string(),
            fragment_path: "fragments/lib.rs.expanded".to_string(),
            extracted_types: vec![],
        },
    );
    m.save(cache_dir).unwrap();
    // Note: the fragment file is deliberately NOT created.

    let loaded = CacheManifest::load(cache_dir, "test_crate");
    assert!(
        loaded.entries.is_empty(),
        "cache with a missing fragment must be discarded"
    );
}

#[test]
fn load_cache_discards_manifest_pointing_at_zero_byte_fragment() {
    // Reproduces the symptom from the original bug: a 0-byte fragment
    // exists on disk and the manifest points at it. Loading must notice
    // the poisoned state and throw the whole thing away.
    let tmp = TempDir::new().unwrap();
    let cache_dir = tmp.path();

    let mut m = CacheManifest::empty("poisoned_crate");
    m.entries.insert(
        "lib.rs".to_string(),
        CacheEntry {
            input_hash: "cafebabe".to_string(),
            module_path: "poisoned_crate".to_string(),
            fragment_path: "fragments/lib.rs.expanded".to_string(),
            extracted_types: vec![],
        },
    );
    m.save(cache_dir).unwrap();

    let frag = cache_dir.join("fragments/lib.rs.expanded");
    fs::create_dir_all(frag.parent().unwrap()).unwrap();
    fs::write(&frag, b"").unwrap(); // 0-byte fragment
    assert_eq!(fs::metadata(&frag).unwrap().len(), 0);

    let loaded = CacheManifest::load(cache_dir, "poisoned_crate");
    assert!(
        loaded.entries.is_empty(),
        "cache with a 0-byte fragment must be discarded"
    );
}

#[test]
fn write_fragment_refuses_empty_contents() {
    // Fix 1b: the low-level write path must refuse empty/whitespace-only
    // fragments. This is belt-and-suspenders on top of 1a.
    let tmp = TempDir::new().unwrap();

    let empty_err = expansion_cache::write_fragment(tmp.path(), "foo.rs", "").unwrap_err();
    assert!(
        empty_err.to_string().contains("empty expansion fragment"),
        "expected empty-fragment error, got: {}",
        empty_err
    );
    assert!(
        !tmp.path().join("fragments/foo.rs.expanded").exists(),
        "no fragment file must be created when the write is rejected"
    );

    let ws_err = expansion_cache::write_fragment(tmp.path(), "bar.rs", "   \n\t").unwrap_err();
    assert!(ws_err.to_string().contains("empty expansion fragment"));
}

#[test]
fn raw_scan_skips_main_rs_and_src_bin() {
    // Even without cargo expand, walk_src's skip logic for main.rs and
    // src/bin/*.rs must apply — otherwise turning on expand_macros would
    // hard-error on every bin-containing crate. This test doesn't turn
    // expand_macros on; it just verifies the scanner finds lib-side types
    // in a crate that also has main.rs + src/bin.
    let tmp = TempDir::new().unwrap();
    let crate_dir = tmp.path().join("mixed_crate");
    fs::create_dir_all(&crate_dir).unwrap();

    fs::write(
        crate_dir.join("Cargo.toml"),
        r#"[package]
name = "mixed_crate"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "mixed_crate"
path = "src/main.rs"
"#,
    )
    .unwrap();

    let src = crate_dir.join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("lib.rs"),
        r#"pub trait EvenframePersistableStruct {}
pub struct LibOnly { pub id: String }
impl EvenframePersistableStruct for LibOnly {}
"#,
    )
    .unwrap();

    // A bin target alongside the lib — `cargo expand --lib` never sees this.
    fs::write(
        src.join("main.rs"),
        r#"fn main() { println!("ignored by scanner"); }"#,
    )
    .unwrap();

    // An extra bin in src/bin — also never seen by cargo expand --lib.
    fs::create_dir_all(src.join("bin")).unwrap();
    fs::write(
        src.join("bin/tool.rs"),
        r#"fn main() { println!("also ignored"); }"#,
    )
    .unwrap();

    // Raw-source scan (expand_macros=false) over the temp crate.
    let scanner =
        WorkspaceScanner::with_path(crate_dir.clone(), Vec::new(), false);
    let types = scanner
        .scan_for_evenframe_types()
        .expect("raw scan must succeed");

    let names: Vec<_> = types.iter().map(|t| t.name.as_str()).collect();
    assert!(
        names.contains(&"LibOnly"),
        "lib types must still be discovered; found: {:?}",
        names
    );
    // Nothing from main.rs / src/bin should end up in the output — they
    // have no Evenframe markers, so the scanner shouldn't find anything
    // there, and we shouldn't have crashed trying to walk them.
}

// ============================================================================
// Tests that DO need cargo expand
// ============================================================================

/// Happy-path expander run. Verifies:
///
/// 1. types are discovered through the expansion cache path,
/// 2. every fragment file written is non-empty (no 0-byte regression),
/// 3. the manifest survives a `CacheManifest::load` round-trip.
#[test]
fn expand_macros_discovers_types_and_writes_no_zero_byte_fragments() {
    if !cargo_expand_available() {
        eprintln!(
            "SKIP expand_macros_discovers_types_and_writes_no_zero_byte_fragments: \
             cargo-expand not installed. Run `cargo install cargo-expand` to enable."
        );
        return;
    }

    let tmp = TempDir::new().unwrap();
    let crate_dir = tmp.path().join("expand_me");
    fs::create_dir_all(&crate_dir).unwrap();
    write_minimal_lib_crate(&crate_dir, "expand_me");

    let scanner =
        WorkspaceScanner::with_path(crate_dir.clone(), Vec::new(), true);
    let types = scanner
        .scan_for_evenframe_types()
        .expect("expansion-mode scan must succeed on a valid temp crate");

    // Should have discovered at least User, Profile, Widget, Color.
    let names: Vec<_> = types.iter().map(|t| t.name.clone()).collect();
    for expected in ["User", "Profile", "Widget", "Color"] {
        assert!(
            names.iter().any(|n| n == expected),
            "expected scanner to find '{}' via expansion; found: {:?}",
            expected,
            names
        );
    }

    // The cache lives under `<crate>/target/.evenframe-expanded/<crate_name>/`.
    // `find_target_dir` walks upward so it'll locate whichever target/ cargo
    // expand created.
    let target_dir = expansion_cache::find_target_dir(&crate_dir);
    let cache_dir = expansion_cache::crate_cache_dir(&target_dir, "expand_me");
    assert!(
        cache_dir.exists(),
        "expansion cache directory was not created at {:?}",
        cache_dir
    );

    // Every file under the cache must be non-empty. A single 0-byte fragment
    // is exactly the regression we're guarding against.
    let sizes = collect_file_sizes(&cache_dir);
    assert!(
        !sizes.is_empty(),
        "cache directory {:?} is empty — no fragments were written",
        cache_dir
    );
    for (path, len) in &sizes {
        assert!(
            *len > 0,
            "found 0-byte cache file at {:?} — this is the regression",
            path
        );
    }

    // The manifest on disk must survive a round-trip through the loader.
    // If any fragment were 0-byte, the loader would have discarded it.
    let loaded = CacheManifest::load(&cache_dir, "expand_me");
    assert!(
        !loaded.entries.is_empty(),
        "persisted manifest was empty after reload — fragments may be corrupt"
    );
}

/// Second-run cache hit. Verifies that running the scanner twice in a row
/// on an unchanged source tree reuses the cached fragments rather than
/// re-expanding. We detect cache reuse by fragment mtimes: if the second
/// run re-wrote a fragment, its mtime would bump.
#[test]
fn expand_macros_second_run_is_a_cache_hit() {
    if !cargo_expand_available() {
        eprintln!("SKIP expand_macros_second_run_is_a_cache_hit: cargo-expand not installed");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let crate_dir = tmp.path().join("cache_me");
    fs::create_dir_all(&crate_dir).unwrap();
    write_minimal_lib_crate(&crate_dir, "cache_me");

    let scanner =
        WorkspaceScanner::with_path(crate_dir.clone(), Vec::new(), true);

    // First run: populates the cache.
    let first = scanner.scan_for_evenframe_types().expect("first scan");
    assert!(!first.is_empty(), "first scan should discover types");

    let target_dir = expansion_cache::find_target_dir(&crate_dir);
    let cache_dir = expansion_cache::crate_cache_dir(&target_dir, "cache_me");
    let first_sizes = collect_file_sizes(&cache_dir);

    // Snapshot fragment mtimes.
    let fragments_dir = cache_dir.join("fragments");
    let first_mtimes: Vec<(PathBuf, std::time::SystemTime)> =
        collect_file_sizes(&fragments_dir)
            .into_iter()
            .map(|(p, _)| {
                let mtime = fs::metadata(&p).unwrap().modified().unwrap();
                (p, mtime)
            })
            .collect();
    assert!(
        !first_mtimes.is_empty(),
        "first run should have produced fragments under {:?}",
        fragments_dir
    );

    // Second run: should re-use the cache. Types must match, fragment
    // mtimes must NOT change (no re-writes).
    let second = scanner.scan_for_evenframe_types().expect("second scan");
    assert_eq!(
        first.len(),
        second.len(),
        "second-run type count diverged from first run"
    );

    let second_sizes = collect_file_sizes(&cache_dir);
    assert_eq!(
        first_sizes.len(),
        second_sizes.len(),
        "cache file count changed between runs"
    );

    for (path, first_mtime) in &first_mtimes {
        let second_mtime = fs::metadata(path).unwrap().modified().unwrap();
        assert_eq!(
            *first_mtime, second_mtime,
            "fragment {:?} was re-written on the second run — cache miss",
            path
        );
    }
}

/// Regression test for fix 1d: a crate that has *both* `src/lib.rs` and
/// `src/main.rs` must not hard-error the expand path. Before the fix,
/// `walk_src` would find `main.rs`, compute a module path for it, and then
/// `split_expanded_by_module` would never produce that key (because
/// `cargo expand --lib` skips the bin target) — tripping the new
/// "missing module" hard error.
#[test]
fn expand_macros_handles_mixed_lib_and_bin_crate() {
    if !cargo_expand_available() {
        eprintln!(
            "SKIP expand_macros_handles_mixed_lib_and_bin_crate: cargo-expand not installed"
        );
        return;
    }

    let tmp = TempDir::new().unwrap();
    let crate_dir = tmp.path().join("mixed");
    fs::create_dir_all(&crate_dir).unwrap();

    fs::write(
        crate_dir.join("Cargo.toml"),
        r#"[package]
name = "mixed"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "mixed"
path = "src/main.rs"
"#,
    )
    .unwrap();

    let src = crate_dir.join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("lib.rs"),
        r#"pub trait EvenframePersistableStruct {}
pub struct Account { pub id: String, pub handle: String }
impl EvenframePersistableStruct for Account {}
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.rs"),
        r#"fn main() {
    println!("hello from the bin target");
}
"#,
    )
    .unwrap();

    // src/bin/*.rs also must be skipped.
    fs::create_dir_all(src.join("bin")).unwrap();
    fs::write(
        src.join("bin/extra.rs"),
        r#"fn main() { println!("extra"); }"#,
    )
    .unwrap();

    let scanner =
        WorkspaceScanner::with_path(crate_dir.clone(), Vec::new(), true);
    let result = scanner.scan_for_evenframe_types();

    // This MUST succeed. Before fix 1d, it would fail with a "missing module"
    // corruption error because `main.rs` would try to look up the crate root
    // module in the expanded output twice (once for lib, once for main).
    let types = result.expect("mixed lib/bin crate must scan cleanly in expand mode");
    assert!(
        types.iter().any(|t| t.name == "Account"),
        "expected to find lib-side `Account` type; got: {:?}",
        types.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // Check the cache doesn't contain a `main.rs.expanded` entry.
    let target_dir = expansion_cache::find_target_dir(&crate_dir);
    let cache_dir = expansion_cache::crate_cache_dir(&target_dir, "mixed");
    let loaded = CacheManifest::load(&cache_dir, "mixed");
    assert!(
        !loaded.entries.contains_key("main.rs"),
        "cache should not have an entry for main.rs; entries: {:?}",
        loaded.entries.keys().collect::<Vec<_>>()
    );
    assert!(
        !loaded.entries.keys().any(|k| k.starts_with("bin/")),
        "cache should not have entries for src/bin/*; entries: {:?}",
        loaded.entries.keys().collect::<Vec<_>>()
    );
}
