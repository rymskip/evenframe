//! Per-file macro expansion cache for [`WorkspaceScanner`].
//!
//! This module provides hash-gated caching of `cargo expand` output on a
//! per-source-file basis. The goal is that editing a single `.rs` file only
//! re-expands that file, not the whole crate.
//!
//! # Layout
//!
//! For a crate named `my_crate`, the cache lives under:
//!
//! ```text
//! <target>/.evenframe-expanded/my_crate/
//!     manifest.json                  -- CacheManifest, source of truth
//!     fragments/
//!         lib.rs.expanded            -- per-file expansion output
//!         foo.rs.expanded
//!         bar/baz.rs.expanded
//! ```
//!
//! The manifest keys entries by each source file's path relative to the
//! crate's `src/` directory, and stores a blake3 hash of the file's bytes.
//! On the next run, unchanged files are served directly from cache.

use crate::error::{EvenframeError, Result};
use crate::tooling::workspace_scanner::EvenframeType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, trace, warn};

/// Current manifest schema version. Bump when the on-disk format changes in
/// an incompatible way — loads of older versions fall back to an empty cache.
pub const MANIFEST_VERSION: u32 = 1;

/// Number of changed files at or above which we switch from per-file
/// `cargo expand` invocations to a single crate-level call.
pub const CRATE_LEVEL_THRESHOLD: usize = 5;

/// Top-level cache manifest, one per crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheManifest {
    pub version: u32,
    pub crate_name: String,
    /// Keyed by source path relative to the crate's `src/` directory,
    /// e.g. `lib.rs`, `foo.rs`, `bar/baz.rs`.
    pub entries: HashMap<String, CacheEntry>,
}

/// A single per-file cache entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Blake3 hex digest of the source file's bytes.
    pub input_hash: String,
    /// Fully-qualified module path (e.g. `my_crate::foo::bar`).
    pub module_path: String,
    /// Fragment path relative to the crate cache directory.
    pub fragment_path: String,
    /// Previously extracted Evenframe types from this file.
    pub extracted_types: Vec<EvenframeType>,
}

impl CacheManifest {
    /// Creates an empty manifest for a given crate.
    pub fn empty(crate_name: &str) -> Self {
        Self {
            version: MANIFEST_VERSION,
            crate_name: crate_name.to_string(),
            entries: HashMap::new(),
        }
    }

    /// Loads the manifest from disk, returning an empty one on any failure
    /// (missing file, parse error, version mismatch).
    ///
    /// Also validates that every referenced fragment file exists and is
    /// non-empty on disk. A 0-byte fragment is treated as cache corruption
    /// (the manifest was saved mid-write, or a buggy writer stored an empty
    /// fragment) — in that case the whole manifest is discarded so the
    /// next run re-expands from scratch.
    pub fn load(cache_dir: &Path, crate_name: &str) -> Self {
        let path = cache_dir.join("manifest.json");
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                trace!("no existing manifest at {:?}: {}", path, e);
                return Self::empty(crate_name);
            }
        };
        let manifest = match serde_json::from_slice::<CacheManifest>(&bytes) {
            Ok(m) if m.version == MANIFEST_VERSION && m.crate_name == crate_name => m,
            Ok(m) => {
                debug!(
                    "manifest at {:?} has mismatched version/crate ({} vs {}, {:?} vs {:?}); \
                     starting fresh",
                    path, m.version, MANIFEST_VERSION, m.crate_name, crate_name
                );
                return Self::empty(crate_name);
            }
            Err(e) => {
                warn!(
                    "failed to parse manifest at {:?}: {}; starting fresh",
                    path, e
                );
                return Self::empty(crate_name);
            }
        };

        // Validate that every referenced fragment exists and is non-empty.
        for (rel_source, entry) in &manifest.entries {
            let abs = cache_dir.join(&entry.fragment_path);
            match fs::metadata(&abs) {
                Ok(md) if md.len() == 0 => {
                    warn!(
                        "expansion cache for crate '{}' contains a 0-byte fragment at {:?} \
                         (referenced by source '{}'); discarding the entire cache",
                        crate_name, abs, rel_source
                    );
                    return Self::empty(crate_name);
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(
                        "expansion cache for crate '{}' references missing fragment {:?} \
                         (source '{}'): {}; discarding the entire cache",
                        crate_name, abs, rel_source, e
                    );
                    return Self::empty(crate_name);
                }
            }
        }

        manifest
    }

    /// Atomically writes the manifest to `cache_dir/manifest.json` via a
    /// temporary file + rename.
    pub fn save(&self, cache_dir: &Path) -> Result<()> {
        fs::create_dir_all(cache_dir)?;
        let final_path = cache_dir.join("manifest.json");
        let tmp_path = cache_dir.join("manifest.json.tmp");
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| EvenframeError::Config(format!("manifest serialize: {}", e)))?;
        fs::write(&tmp_path, &bytes)?;
        fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }
}

/// Returns the cache directory for a given crate.
pub fn crate_cache_dir(target_dir: &Path, crate_name: &str) -> PathBuf {
    target_dir.join(".evenframe-expanded").join(crate_name)
}

/// Walks up from `start` to find an existing `target/` directory. Falls back
/// to `start.join("target")` if none is found.
pub fn find_target_dir(start: &Path) -> PathBuf {
    let mut current = start.to_path_buf();
    loop {
        let target = current.join("target");
        if target.is_dir() {
            return target;
        }
        if !current.pop() {
            return start.join("target");
        }
    }
}

/// Computes the blake3 hex digest of the file at `path`.
pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

/// Runs `cargo expand --lib [crate::module::path]` for a single file and
/// returns the expanded source, or `None` on failure.
///
/// When `module_path` equals the crate root (`crate_name`) or is empty, no
/// path argument is passed — this expands the crate root.
pub fn expand_file(manifest_dir: &Path, crate_name: &str, module_path: &str) -> Option<String> {
    let mut cmd = Command::new("cargo");
    cmd.arg("expand").arg("--lib").arg("--theme=none");

    // Strip the leading `{crate_name}::` from the module path if present;
    // cargo expand wants the in-crate path only.
    let inner = module_path
        .strip_prefix(&format!("{}::", crate_name))
        .unwrap_or(module_path);
    if !inner.is_empty() && inner != crate_name {
        cmd.arg(inner);
    }
    cmd.current_dir(manifest_dir);

    match cmd.output() {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).to_string();
            debug!("cargo expand {}::{} → {} bytes", crate_name, inner, s.len());
            Some(s)
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            warn!(
                "cargo expand {}::{} failed: {}",
                crate_name,
                inner,
                stderr.trim()
            );
            None
        }
        Err(e) => {
            warn!(
                "failed to spawn cargo expand for {}::{}: {}",
                crate_name, inner, e
            );
            None
        }
    }
}

/// Runs `cargo expand --lib` over the whole crate and returns the expanded
/// source. Used as the bulk-path expansion when many files need to be
/// re-expanded (see [`CRATE_LEVEL_THRESHOLD`]).
pub fn expand_crate_full(manifest_dir: &Path, crate_name: &str) -> Option<String> {
    let output = Command::new("cargo")
        .arg("expand")
        .arg("--lib")
        .arg("--theme=none")
        .current_dir(manifest_dir)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).to_string();
            debug!(
                "cargo expand (full crate {}) → {} bytes",
                crate_name,
                s.len()
            );
            Some(s)
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("no such subcommand") || stderr.contains("not found") {
                warn!(
                    "cargo-expand is not installed. Install with: cargo install cargo-expand. \
                     Falling back to raw source scanning for crate '{}'.",
                    crate_name
                );
            } else {
                warn!(
                    "cargo expand failed for crate '{}': {}",
                    crate_name,
                    stderr.trim()
                );
            }
            None
        }
        Err(e) => {
            warn!(
                "failed to spawn cargo expand for crate '{}': {}",
                crate_name, e
            );
            None
        }
    }
}

/// Walks nested `Item::Mod` blocks in a parsed crate-level expansion and
/// returns a map from fully-qualified module path to the serialized source
/// of the items *directly* at that module level (nested modules are handled
/// as separate entries, not inlined).
///
/// The emitted source uses `quote::ToTokens` — it's parseable by
/// `syn::parse_file` and that's all the downstream scanner needs. It is not
/// intended to be human-readable.
pub fn split_expanded_by_module(parsed: &syn::File, crate_name: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    out.insert(
        crate_name.to_string(),
        items_to_source_shallow(&parsed.items),
    );
    walk_mods(&parsed.items, crate_name, &mut out);
    out
}

fn walk_mods(items: &[syn::Item], module_path: &str, out: &mut HashMap<String, String>) {
    for item in items {
        if let syn::Item::Mod(m) = item
            && let Some((_, mod_items)) = &m.content
        {
            let child = format!("{}::{}", module_path, m.ident);
            out.insert(child.clone(), items_to_source_shallow(mod_items));
            walk_mods(mod_items, &child, out);
        }
    }
}

/// Serializes a slice of items back to Rust source, skipping nested modules
/// with inline content (those are emitted separately by [`walk_mods`]).
fn items_to_source_shallow(items: &[syn::Item]) -> String {
    use quote::ToTokens;
    let mut s = String::new();
    for item in items {
        if let syn::Item::Mod(m) = item
            && m.content.is_some()
        {
            // Emit just `mod name;` as a placeholder so the fragment parses.
            s.push_str("mod ");
            s.push_str(&m.ident.to_string());
            s.push_str(";\n");
            continue;
        }
        s.push_str(&item.to_token_stream().to_string());
        s.push('\n');
    }
    s
}

/// Writes an expanded fragment to disk under the crate cache directory.
/// Returns the fragment path relative to `cache_dir`.
///
/// Refuses to write an empty (or whitespace-only) fragment. Empty fragments
/// poison the cache: on the next run they get re-loaded as valid hits with
/// `extracted_types: []`, silently dropping the types the user expected.
/// Callers that have legitimately-empty expansions must not reach this path.
pub fn write_fragment(cache_dir: &Path, rel_source_path: &str, contents: &str) -> Result<String> {
    if contents.trim().is_empty() {
        return Err(EvenframeError::WorkspaceScan(format!(
            "refusing to write empty expansion fragment for '{}' — this would poison \
             the cache. Either the module is missing from `cargo expand` output or the \
             upstream split produced an empty body.",
            rel_source_path
        )));
    }
    let rel_fragment = format!("fragments/{}.expanded", rel_source_path);
    let abs = cache_dir.join(&rel_fragment);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&abs, contents)?;
    Ok(rel_fragment)
}

/// Reads a fragment back from disk.
pub fn read_fragment(cache_dir: &Path, rel_fragment: &str) -> Result<String> {
    let abs = cache_dir.join(rel_fragment);
    Ok(fs::read_to_string(abs)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn hash_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("a.rs");
        fs::write(&p, b"hello world").unwrap();
        let h1 = hash_file(&p).unwrap();
        let h2 = hash_file(&p).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // blake3 hex is 64 chars
    }

    #[test]
    fn hash_file_differs_on_change() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("a.rs");
        fs::write(&p, b"hello").unwrap();
        let h1 = hash_file(&p).unwrap();
        fs::write(&p, b"world").unwrap();
        let h2 = hash_file(&p).unwrap();
        assert_ne!(h1, h2);
    }

    #[test]
    fn manifest_save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path();
        let mut m = CacheManifest::empty("my_crate");
        m.entries.insert(
            "lib.rs".to_string(),
            CacheEntry {
                input_hash: "deadbeef".to_string(),
                module_path: "my_crate".to_string(),
                fragment_path: "fragments/lib.rs.expanded".to_string(),
                extracted_types: vec![],
            },
        );
        m.save(cache_dir).unwrap();
        // load() now validates referenced fragments — create a non-empty one.
        write_fragment(cache_dir, "lib.rs", "struct Placeholder;").unwrap();

        let loaded = CacheManifest::load(cache_dir, "my_crate");
        assert_eq!(loaded.version, MANIFEST_VERSION);
        assert_eq!(loaded.crate_name, "my_crate");
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries["lib.rs"].input_hash, "deadbeef");
    }

    #[test]
    fn manifest_load_discards_cache_when_fragment_missing() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path();
        let mut m = CacheManifest::empty("my_crate");
        m.entries.insert(
            "lib.rs".to_string(),
            CacheEntry {
                input_hash: "deadbeef".to_string(),
                module_path: "my_crate".to_string(),
                fragment_path: "fragments/lib.rs.expanded".to_string(),
                extracted_types: vec![],
            },
        );
        m.save(cache_dir).unwrap();
        // Deliberately do NOT create the fragment file.

        let loaded = CacheManifest::load(cache_dir, "my_crate");
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn manifest_load_discards_cache_when_fragment_empty() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path();
        let mut m = CacheManifest::empty("my_crate");
        m.entries.insert(
            "lib.rs".to_string(),
            CacheEntry {
                input_hash: "deadbeef".to_string(),
                module_path: "my_crate".to_string(),
                fragment_path: "fragments/lib.rs.expanded".to_string(),
                extracted_types: vec![],
            },
        );
        m.save(cache_dir).unwrap();
        // Bypass write_fragment's empty-guard to simulate a corrupt on-disk fragment.
        let frag = cache_dir.join("fragments/lib.rs.expanded");
        fs::create_dir_all(frag.parent().unwrap()).unwrap();
        fs::write(&frag, b"").unwrap();

        let loaded = CacheManifest::load(cache_dir, "my_crate");
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn write_fragment_rejects_empty_contents() {
        let dir = TempDir::new().unwrap();
        let err = write_fragment(dir.path(), "foo.rs", "").unwrap_err();
        assert!(err.to_string().contains("empty expansion fragment"));
        // No fragment file should have been created.
        assert!(!dir.path().join("fragments/foo.rs.expanded").exists());
    }

    #[test]
    fn write_fragment_rejects_whitespace_only() {
        let dir = TempDir::new().unwrap();
        assert!(write_fragment(dir.path(), "foo.rs", "   \n\t").is_err());
    }

    #[test]
    fn manifest_load_returns_empty_on_missing_file() {
        let dir = TempDir::new().unwrap();
        let loaded = CacheManifest::load(dir.path(), "my_crate");
        assert_eq!(loaded.version, MANIFEST_VERSION);
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn manifest_load_returns_empty_on_crate_mismatch() {
        let dir = TempDir::new().unwrap();
        let m = CacheManifest::empty("old_crate");
        m.save(dir.path()).unwrap();

        let loaded = CacheManifest::load(dir.path(), "new_crate");
        assert_eq!(loaded.crate_name, "new_crate");
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn manifest_save_is_atomic() {
        let dir = TempDir::new().unwrap();
        let m = CacheManifest::empty("my_crate");
        m.save(dir.path()).unwrap();
        // The .tmp file should not exist after a successful save.
        assert!(!dir.path().join("manifest.json.tmp").exists());
        assert!(dir.path().join("manifest.json").exists());
    }

    #[test]
    fn split_expanded_by_module_flattens_nested_mods() {
        let source = r#"
            struct A;
            mod foo {
                struct B;
                mod bar {
                    struct C;
                }
            }
            struct D;
        "#;
        let parsed = syn::parse_file(source).unwrap();
        let out = split_expanded_by_module(&parsed, "my_crate");

        assert!(out.contains_key("my_crate"));
        assert!(out.contains_key("my_crate::foo"));
        assert!(out.contains_key("my_crate::foo::bar"));

        // Root contains A and D but not B or C.
        let root = &out["my_crate"];
        assert!(root.contains("struct A"));
        assert!(root.contains("struct D"));
        assert!(!root.contains("struct B"));
        assert!(!root.contains("struct C"));

        // foo contains B but not C.
        let foo = &out["my_crate::foo"];
        assert!(foo.contains("struct B"));
        assert!(!foo.contains("struct C"));

        // foo::bar contains C.
        let bar = &out["my_crate::foo::bar"];
        assert!(bar.contains("struct C"));
    }

    #[test]
    fn split_expanded_survives_syn_reparse() {
        // After splitting, each fragment must be re-parseable by syn.
        let source = r#"
            #[derive(Debug)]
            pub struct Foo { pub id: String }
            mod bar {
                pub enum Baz { A, B }
            }
        "#;
        let parsed = syn::parse_file(source).unwrap();
        let out = split_expanded_by_module(&parsed, "root");
        for (path, src) in &out {
            syn::parse_file(src)
                .unwrap_or_else(|e| panic!("failed to reparse fragment for {}: {}", path, e));
        }
    }

    #[test]
    fn write_and_read_fragment_roundtrip() {
        let dir = TempDir::new().unwrap();
        let rel = write_fragment(dir.path(), "foo/bar.rs", "struct X;").unwrap();
        assert_eq!(rel, "fragments/foo/bar.rs.expanded");
        assert!(dir.path().join(&rel).exists());
        let contents = read_fragment(dir.path(), &rel).unwrap();
        assert_eq!(contents, "struct X;");
    }
}
