//! Per-file macro expansion cache for [`WorkspaceScanner`](super::WorkspaceScanner).
//!
//! This module provides hash-gated caching of `cargo expand` output on a
//! per-source-file basis. Any changed file costs one expansion of the whole
//! crate; unchanged files are served from their recorded types.
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
use crate::scan::configs::ParsedType;
use crate::scan::paths::ModuleScope;
use crate::scan::workspace::{EvenframeType, Scan, ScannedItem};
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, trace, warn};

/// Current manifest schema version. Bump when the on-disk format changes in
/// an incompatible way; loads of older versions fall back to an empty cache.
pub const MANIFEST_VERSION: u32 = 9;

/// Top-level cache manifest, one per crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheManifest {
    pub version: u32,
    pub crate_name: String,
    /// Absolute path of the crate's `src/` directory, which `entries` are
    /// relative to.
    pub src_dir: PathBuf,
    /// The `apply_aliases` the entries' types were extracted under. Which
    /// types an alias makes Evenframe types changes with them.
    pub apply_aliases: Vec<String>,
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
    /// Fragment path relative to the crate cache directory, or `None` for
    /// a file that contributes no items.
    pub fragment_path: Option<String>,
    /// The Evenframe types extracted from this file.
    pub items: Vec<CachedItem>,
    /// The scopes of the file's modules, by module path.
    pub scopes: BTreeMap<String, ModuleScope>,
}

/// An extracted type with its parsed configuration, or `None` when parsing
/// failed, so the failure is reported again from the fragment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedItem {
    pub evenframe_type: EvenframeType,
    pub parsed: Option<ParsedType>,
}

impl CacheEntry {
    /// The entry for a fragment and what was scanned from it.
    pub fn new(
        input_hash: String,
        module_path: String,
        fragment_path: Option<String>,
        scan: &Scan,
    ) -> Self {
        Self {
            input_hash,
            module_path,
            fragment_path,
            scopes: scan.scopes.clone(),
            items: scan
                .items
                .iter()
                .map(|item| CachedItem {
                    evenframe_type: item.evenframe_type.clone(),
                    parsed: item.parsed.as_ref().ok().cloned(),
                })
                .collect(),
        }
    }

    /// The recorded scan, or `None` when one of its types has to be parsed
    /// again.
    pub fn recorded_scan(&self) -> Option<Scan> {
        let items = self
            .items
            .iter()
            .map(|item| {
                item.parsed.clone().map(|parsed| ScannedItem {
                    evenframe_type: item.evenframe_type.clone(),
                    parsed: Ok(parsed),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Scan {
            items,
            scopes: self.scopes.clone(),
        })
    }
}

/// Just the version of a manifest, readable whatever its layout.
#[derive(Deserialize)]
struct ManifestVersion {
    version: u32,
}

impl CacheManifest {
    /// Creates an empty manifest for the crate whose sources are in `src_dir`,
    /// scanned with `apply_aliases`.
    pub fn empty(crate_name: &str, src_dir: &Path, apply_aliases: &[String]) -> Self {
        Self {
            version: MANIFEST_VERSION,
            crate_name: crate_name.to_string(),
            src_dir: src_dir.to_path_buf(),
            apply_aliases: apply_aliases.to_vec(),
            entries: HashMap::new(),
        }
    }

    /// Loads the manifest from disk, or `None` when there is no usable one
    /// (missing file, parse error, version or crate mismatch).
    ///
    /// Also validates that every referenced fragment file exists and is
    /// non-empty on disk. A 0-byte fragment is treated as cache corruption
    /// (the manifest was saved mid-write, or a buggy writer stored an empty
    /// fragment). In that case the whole manifest is discarded so the
    /// next run re-expands from scratch.
    pub fn load(cache_dir: &Path, crate_name: &str) -> Option<Self> {
        let path = cache_dir.join("manifest.json");
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                trace!("no existing manifest at {:?}: {}", path, e);
                return None;
            }
        };
        // The version is read on its own first, so a manifest in an older
        // layout is a version mismatch rather than a parse failure.
        match serde_json::from_slice::<ManifestVersion>(&bytes) {
            Ok(v) if v.version == MANIFEST_VERSION => {}
            Ok(v) => {
                debug!(
                    "manifest at {:?} is version {} (expected {}); starting fresh",
                    path, v.version, MANIFEST_VERSION
                );
                return None;
            }
            Err(e) => {
                warn!(
                    "failed to parse manifest at {:?}: {}; starting fresh",
                    path, e
                );
                return None;
            }
        }
        let manifest = match serde_json::from_slice::<CacheManifest>(&bytes) {
            Ok(m) if m.crate_name == crate_name => m,
            Ok(m) => {
                debug!(
                    "manifest at {:?} belongs to crate {:?}, not {:?}; starting fresh",
                    path, m.crate_name, crate_name
                );
                return None;
            }
            Err(e) => {
                warn!(
                    "failed to parse manifest at {:?}: {}; starting fresh",
                    path, e
                );
                return None;
            }
        };

        // Validate that every referenced fragment exists and is non-empty.
        for (rel_source, entry) in &manifest.entries {
            let Some(fragment_path) = &entry.fragment_path else {
                continue;
            };
            let abs = cache_dir.join(fragment_path);
            match fs::metadata(&abs) {
                Ok(md) if md.len() == 0 => {
                    warn!(
                        "expansion cache for crate '{}' contains a 0-byte fragment at {:?} \
                         (referenced by source '{}'); discarding the entire cache",
                        crate_name, abs, rel_source
                    );
                    return None;
                }
                Ok(_) => {}
                Err(e) => {
                    warn!(
                        "expansion cache for crate '{}' references missing fragment {:?} \
                         (source '{}'): {}; discarding the entire cache",
                        crate_name, abs, rel_source, e
                    );
                    return None;
                }
            }
        }

        Some(manifest)
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

/// Runs `cargo expand --lib` over the crate in `manifest_dir`. One
/// expansion serves every changed file: cargo-expand compiles and expands the
/// whole crate even when asked for a single module.
pub fn expand_crate(manifest_dir: &Path, crate_name: &str) -> Result<String> {
    let output = Command::new("cargo")
        .args(["expand", "--lib", "--theme=none"])
        .current_dir(manifest_dir)
        .output()
        .map_err(|error| {
            EvenframeError::WorkspaceScan(format!(
                "failed to run cargo expand for crate '{crate_name}': {error}"
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let hint = if stderr.contains("no such command") || stderr.contains("no such subcommand") {
            " Install it with `cargo install cargo-expand`, or disable `expand_macros` in \
             evenframe.toml."
        } else {
            ""
        };
        return Err(EvenframeError::WorkspaceScan(format!(
            "cargo expand failed for crate '{crate_name}': {}{hint}",
            stderr.trim()
        )));
    }
    let expanded = String::from_utf8(output.stdout).map_err(|error| {
        EvenframeError::WorkspaceScan(format!(
            "cargo expand printed invalid UTF-8 for crate '{crate_name}': {error}"
        ))
    })?;
    debug!(
        "cargo expand (crate {}) printed {} bytes",
        crate_name,
        expanded.len()
    );
    Ok(expanded)
}

/// Splits a parsed crate expansion into the items each source file
/// contributes, keyed by the file's module path (`file_modules`). An inline
/// module that is another file's moves to that file's entry and leaves a
/// `mod name;` placeholder behind; any other inline module stays with the
/// file that defines it.
pub fn split_by_file(
    items: Vec<syn::Item>,
    crate_name: &str,
    file_modules: &HashSet<String>,
) -> HashMap<String, Vec<syn::Item>> {
    let mut by_file = HashMap::new();
    split_into(items, crate_name.to_string(), file_modules, &mut by_file);
    by_file
}

fn split_into(
    items: Vec<syn::Item>,
    module_path: String,
    file_modules: &HashSet<String>,
    by_file: &mut HashMap<String, Vec<syn::Item>>,
) {
    let mut own = Vec::with_capacity(items.len());
    for item in items {
        match item {
            syn::Item::Mod(mut module) => {
                let child = format!("{module_path}::{}", module.ident);
                if file_modules.contains(&child)
                    && let Some((_, child_items)) = module.content.take()
                {
                    split_into(child_items, child, file_modules, by_file);
                    module.semi = Some(syn::token::Semi::default());
                }
                own.push(syn::Item::Mod(module));
            }
            item => own.push(item),
        }
    }
    by_file.insert(module_path, own);
}

/// The source of a file's expanded items, as its fragment stores them.
pub fn fragment_source(items: &[syn::Item]) -> String {
    items
        .iter()
        .map(|item| format!("{}\n", item.to_token_stream()))
        .collect()
}

/// Writes an expanded fragment to disk under the crate cache directory.
/// Returns the fragment path relative to `cache_dir`.
///
/// Refuses to write an empty (or whitespace-only) fragment. Empty fragments
/// poison the cache: on the next run they get re-loaded as valid hits with
/// `items: []`, silently dropping the types the user expected.
/// Callers that have legitimately-empty expansions must not reach this path.
pub fn write_fragment(cache_dir: &Path, rel_source_path: &str, contents: &str) -> Result<String> {
    if contents.trim().is_empty() {
        return Err(EvenframeError::WorkspaceScan(format!(
            "refusing to write empty expansion fragment for '{}': it would poison \
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

#[cfg(test)]
mod tests {
    use super::{
        BTreeMap, CacheEntry, CacheManifest, HashSet, MANIFEST_VERSION, Path, fragment_source, fs,
        hash_file, split_by_file, write_fragment,
    };
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
        let mut m = CacheManifest::empty("my_crate", Path::new("/proj/src"), &[]);
        m.entries.insert(
            "lib.rs".to_string(),
            CacheEntry {
                input_hash: "deadbeef".to_string(),
                module_path: "my_crate".to_string(),
                fragment_path: Some("fragments/lib.rs.expanded".to_string()),
                items: vec![],
                scopes: BTreeMap::new(),
            },
        );
        m.save(cache_dir).unwrap();
        // load() validates referenced fragments, so create a non-empty one.
        write_fragment(cache_dir, "lib.rs", "struct Placeholder;").unwrap();

        let loaded = CacheManifest::load(cache_dir, "my_crate").unwrap();
        assert_eq!(loaded.version, MANIFEST_VERSION);
        assert_eq!(loaded.crate_name, "my_crate");
        assert_eq!(loaded.src_dir, Path::new("/proj/src"));
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries["lib.rs"].input_hash, "deadbeef");
    }

    #[test]
    fn manifest_load_discards_cache_when_fragment_missing() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path();
        let mut m = CacheManifest::empty("my_crate", Path::new("/proj/src"), &[]);
        m.entries.insert(
            "lib.rs".to_string(),
            CacheEntry {
                input_hash: "deadbeef".to_string(),
                module_path: "my_crate".to_string(),
                fragment_path: Some("fragments/lib.rs.expanded".to_string()),
                items: vec![],
                scopes: BTreeMap::new(),
            },
        );
        m.save(cache_dir).unwrap();
        // Deliberately do NOT create the fragment file.

        assert!(CacheManifest::load(cache_dir, "my_crate").is_none());
    }

    #[test]
    fn manifest_load_discards_cache_when_fragment_empty() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path();
        let mut m = CacheManifest::empty("my_crate", Path::new("/proj/src"), &[]);
        m.entries.insert(
            "lib.rs".to_string(),
            CacheEntry {
                input_hash: "deadbeef".to_string(),
                module_path: "my_crate".to_string(),
                fragment_path: Some("fragments/lib.rs.expanded".to_string()),
                items: vec![],
                scopes: BTreeMap::new(),
            },
        );
        m.save(cache_dir).unwrap();
        // Bypass write_fragment's empty-guard to simulate a corrupt on-disk fragment.
        let frag = cache_dir.join("fragments/lib.rs.expanded");
        fs::create_dir_all(frag.parent().unwrap()).unwrap();
        fs::write(&frag, b"").unwrap();

        assert!(CacheManifest::load(cache_dir, "my_crate").is_none());
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
    fn manifest_load_returns_none_on_missing_file() {
        let dir = TempDir::new().unwrap();
        assert!(CacheManifest::load(dir.path(), "my_crate").is_none());
    }

    #[test]
    fn manifest_load_returns_none_on_older_version() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            r#"{"version":1,"crate_name":"my_crate","entries":{}}"#,
        )
        .unwrap();
        assert!(CacheManifest::load(dir.path(), "my_crate").is_none());
    }

    #[test]
    fn manifest_load_returns_none_on_crate_mismatch() {
        let dir = TempDir::new().unwrap();
        let m = CacheManifest::empty("old_crate", Path::new("/proj/src"), &[]);
        m.save(dir.path()).unwrap();

        assert!(CacheManifest::load(dir.path(), "new_crate").is_none());
    }

    #[test]
    fn manifest_save_is_atomic() {
        let dir = TempDir::new().unwrap();
        let m = CacheManifest::empty("my_crate", Path::new("/proj/src"), &[]);
        m.save(dir.path()).unwrap();
        // The .tmp file should not exist after a successful save.
        assert!(!dir.path().join("manifest.json.tmp").exists());
        assert!(dir.path().join("manifest.json").exists());
    }

    #[test]
    fn split_by_file_keeps_inline_modules_with_their_file() {
        let source = r#"
            struct A;
            mod foo {
                struct B;
                mod inline {
                    struct C;
                }
                mod bar {
                    struct D;
                }
            }
        "#;
        let parsed = syn::parse_file(source).unwrap();
        let file_modules: HashSet<String> = ["my_crate", "my_crate::foo", "my_crate::foo::bar"]
            .into_iter()
            .map(String::from)
            .collect();
        let by_file = split_by_file(parsed.items, "my_crate", &file_modules);
        assert_eq!(by_file.len(), 3);

        let source_of = |module: &str| fragment_source(&by_file[module]);
        let root = source_of("my_crate");
        assert!(root.contains("struct A"));
        assert!(root.contains("mod foo ;"));
        assert!(!root.contains("struct B"));

        let foo = source_of("my_crate::foo");
        assert!(foo.contains("struct B"));
        assert!(foo.contains("mod inline"));
        assert!(foo.contains("struct C"));
        assert!(foo.contains("mod bar ;"));
        assert!(!foo.contains("struct D"));

        assert!(source_of("my_crate::foo::bar").contains("struct D"));
        for module in file_modules {
            syn::parse_file(&source_of(&module))
                .unwrap_or_else(|error| panic!("the fragment of {module} does not parse: {error}"));
        }
    }

    #[test]
    fn a_file_with_no_items_splits_to_an_empty_entry() {
        let parsed = syn::parse_file("mod empty {}").unwrap();
        let file_modules: HashSet<String> = ["root", "root::empty"]
            .into_iter()
            .map(String::from)
            .collect();
        let by_file = split_by_file(parsed.items, "root", &file_modules);
        assert!(by_file["root::empty"].is_empty());
    }

    #[test]
    fn a_manifest_entry_without_a_fragment_loads() {
        let dir = TempDir::new().unwrap();
        let mut manifest = CacheManifest::empty("my_crate", Path::new("/proj/src"), &[]);
        manifest.entries.insert(
            "empty.rs".to_string(),
            CacheEntry {
                input_hash: "deadbeef".to_string(),
                module_path: "my_crate::empty".to_string(),
                fragment_path: None,
                items: vec![],
                scopes: BTreeMap::new(),
            },
        );
        manifest.save(dir.path()).unwrap();
        assert!(CacheManifest::load(dir.path(), "my_crate").is_some());
    }

    #[test]
    fn write_fragment_roundtrip() {
        let dir = TempDir::new().unwrap();
        let rel = write_fragment(dir.path(), "foo/bar.rs", "struct X;").unwrap();
        assert_eq!(rel, "fragments/foo/bar.rs.expanded");
        assert_eq!(
            fs::read_to_string(dir.path().join(&rel)).unwrap(),
            "struct X;"
        );
    }
}
