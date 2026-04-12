//! Workspace scanning for finding Rust types with Evenframe derives.

use crate::error::{EvenframeError, Result};
use crate::tooling::expansion_cache::{self, CRATE_LEVEL_THRESHOLD, CacheEntry, CacheManifest};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use syn::{Attribute, Item, ItemImpl, Meta, parse_file};
use tracing::{debug, info, trace, warn};
use walkdir::WalkDir;

/// Represents a type found with Evenframe derives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvenframeType {
    /// The name of the type.
    pub name: String,
    /// The module path where the type is defined.
    pub module_path: String,
    /// The file path where the type is defined.
    pub file_path: String,
    /// Whether this is a struct or enum.
    pub kind: TypeKind,
    /// Whether this struct has an `id` field (makes it a table).
    pub has_id_field: bool,
    /// Which pipeline(s) this type participates in.
    pub pipeline: crate::types::Pipeline,
}

impl EvenframeType {
    /// Returns the fully qualified name (module path + name).
    pub fn qualified_name(&self) -> String {
        format!("{}::{}", self.module_path, self.name)
    }
}

impl fmt::Display for EvenframeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.qualified_name())
    }
}

/// The kind of type (struct or enum).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Enum,
}

/// A struct/enum discovered during scanning, pending resolution against
/// manual trait impls found elsewhere in the crate.
#[derive(Debug, Clone)]
struct PendingType {
    ident: String,
    file_path: String,
    module_path: String,
    kind: TypeKind,
    has_id_field: bool,
    /// Pipeline determined by a local `#[derive(...)]` or `#[apply(...)]`.
    /// `None` means the type only qualifies if a manual impl is found
    /// elsewhere in the crate.
    local_pipeline: Option<crate::types::Pipeline>,
}

/// State accumulated while scanning a crate (or a single file, in the test
/// path). A final [`Self::finalize`] merges pending types against manual
/// impls to produce the public `Vec<EvenframeType>`.
#[derive(Debug, Default)]
struct CrateScanState {
    pending: Vec<PendingType>,
    /// type ident → pipeline from a manual `impl EvenframeXxx for T` block.
    manual_impls: HashMap<String, crate::types::Pipeline>,
}

impl CrateScanState {
    fn finalize(self) -> Vec<EvenframeType> {
        let CrateScanState {
            pending,
            manual_impls,
        } = self;
        pending
            .into_iter()
            .filter_map(|p| {
                let pipeline = p
                    .local_pipeline
                    .or_else(|| manual_impls.get(&p.ident).copied());
                pipeline.map(|pipe| EvenframeType {
                    name: p.ident,
                    module_path: p.module_path,
                    file_path: p.file_path,
                    kind: p.kind,
                    has_id_field: p.has_id_field,
                    pipeline: pipe,
                })
            })
            .collect()
    }
}

/// Scanner for finding Evenframe types in a Rust workspace.
pub struct WorkspaceScanner {
    start_path: PathBuf,
    apply_aliases: Vec<String>,
    expand_macros: bool,
}

impl WorkspaceScanner {
    /// Creates a new WorkspaceScanner that starts scanning from the current directory.
    ///
    /// # Arguments
    ///
    /// * `apply_aliases` - A list of attribute aliases to look for.
    /// * `expand_macros` - Whether to run `cargo expand` before scanning.
    pub fn new(apply_aliases: Vec<String>, expand_macros: bool) -> Result<Self> {
        let start_path = env::current_dir()?;
        Ok(Self::with_path(start_path, apply_aliases, expand_macros))
    }

    /// Creates a new WorkspaceScanner with a specific start path.
    ///
    /// # Arguments
    ///
    /// * `start_path` - The directory to start scanning from. It will search this
    ///   directory and its children for Rust workspaces or standalone crates.
    /// * `apply_aliases` - A list of attribute aliases to look for.
    /// * `expand_macros` - Whether to run `cargo expand` before scanning.
    pub fn with_path(start_path: PathBuf, apply_aliases: Vec<String>, expand_macros: bool) -> Self {
        Self {
            start_path,
            apply_aliases,
            expand_macros,
        }
    }

    /// Scans for Rust workspaces and collects all Evenframe types within them.
    ///
    /// Top-level crates are processed in parallel via rayon; each crate gets
    /// its own isolated scan state.
    pub fn scan_for_evenframe_types(&self) -> Result<Vec<EvenframeType>> {
        info!(
            "Starting workspace scan for Evenframe types from path: {:?}",
            self.start_path
        );

        // First, collect all manifests we'll process. This has to be
        // sequential because it consults a dedupe set, but it's a cheap walk.
        let mut manifests: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        for entry in WalkDir::new(&self.start_path)
            .into_iter()
            .filter_map(|e: std::result::Result<walkdir::DirEntry, walkdir::Error>| e.ok())
            .filter(|e: &walkdir::DirEntry| e.file_name() == "Cargo.toml")
        {
            let p = entry.path().to_path_buf();
            if seen.insert(p.clone()) {
                trace!("Found potential manifest: {:?}", p);
                manifests.push(p);
            }
        }

        // Processing strategy depends on whether we're running `cargo expand`:
        //
        // - In `expand_macros` mode, each crate spawns `cargo expand --lib`
        //   which acquires the cargo build lock on `target/`. Running those
        //   in parallel makes them block on each other and can interleave
        //   builds in ways that corrupt the expansion output. Process
        //   sequentially. We also propagate errors instead of downgrading
        //   them to empty results — a corrupt expansion cache should be
        //   visible, not silently papered over.
        //
        // - In the raw-source path, there's no cargo contention and one
        //   broken manifest shouldn't kill the whole scan. Keep the legacy
        //   parallel + log-and-continue behavior.
        let types: Vec<EvenframeType> = if self.expand_macros {
            let mut all = Vec::new();
            for manifest_path in &manifests {
                let v = self.process_manifest(manifest_path).map_err(|e| {
                    EvenframeError::WorkspaceScan(format!(
                        "expansion-mode scan failed at {:?}: {}",
                        manifest_path, e
                    ))
                })?;
                all.extend(v);
            }
            all
        } else {
            manifests
                .par_iter()
                .map(|manifest_path| match self.process_manifest(manifest_path) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("Failed to process manifest at {:?}: {}", manifest_path, e);
                        Vec::new()
                    }
                })
                .collect::<Vec<Vec<EvenframeType>>>()
                .into_iter()
                .flatten()
                .collect()
        };

        info!(
            "Workspace scan complete. Found {} Evenframe types",
            types.len()
        );
        debug!(
            "Type breakdown: {} structs, {} enums",
            types.iter().filter(|t| t.kind == TypeKind::Struct).count(),
            types.iter().filter(|t| t.kind == TypeKind::Enum).count()
        );

        Ok(types)
    }

    /// Processes a Cargo.toml file, determines if it's a workspace or a single
    /// crate, and scans the corresponding source files. Returns the types
    /// found for this manifest only.
    fn process_manifest(&self, manifest_path: &Path) -> Result<Vec<EvenframeType>> {
        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| EvenframeError::InvalidPath {
                path: manifest_path.to_path_buf(),
            })?;

        let content = fs::read_to_string(manifest_path)?;
        let manifest: toml::Value = toml::from_str(&content)
            .map_err(|e| EvenframeError::parse_error(manifest_path, e.to_string()))?;

        let mut out: Vec<EvenframeType> = Vec::new();

        // Check if this is a workspace manifest and scan its members.
        if let Some(workspace) = manifest.get("workspace").and_then(|w| w.as_table())
            && let Some(members) = workspace.get("members").and_then(|m| m.as_array())
        {
            debug!("Processing workspace at: {:?}", manifest_dir);

            for member in members.iter().filter_map(|v| v.as_str()) {
                // Note: For a full implementation, you might use the `glob` crate
                // to handle patterns like "crates/*". This example handles direct paths.
                let member_path = manifest_dir.join(member);
                if member_path.is_dir() {
                    let crate_name = member_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown_crate");
                    let src_path = member_path.join("src");
                    if src_path.exists() {
                        info!(
                            "Scanning workspace member: {} at {:?}",
                            crate_name, src_path
                        );
                        let mut state = CrateScanState::default();
                        self.scan_directory_into(&src_path, &mut state, crate_name, 0)?;
                        out.extend(state.finalize());
                    } else {
                        warn!(
                            "Workspace member '{}' does not have a 'src' directory.",
                            member
                        );
                    }
                } else {
                    warn!(
                        "Workspace member path '{}' is not a directory or does not exist.",
                        member
                    );
                }
            }
        }

        // Also check if this manifest has a [package] section (handles both standalone crates
        // and the case where a crate has an empty [workspace] to exclude from parent workspace).
        if manifest.get("package").is_some() {
            debug!("Processing package at: {:?}", manifest_dir);
            let crate_name = manifest
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or_else(|| {
                    manifest_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown_crate")
                });

            if self.expand_macros {
                let types = self.scan_with_expansion_cache(manifest_dir, crate_name)?;
                out.extend(types);
                return Ok(out);
            }

            let src_path = manifest_dir.join("src");
            if src_path.exists() {
                info!("Scanning crate: {} at {:?}", crate_name, src_path);
                let mut state = CrateScanState::default();
                self.scan_directory_into(&src_path, &mut state, crate_name, 0)?;
                out.extend(state.finalize());
            }
        }

        Ok(out)
    }

    /// Expansion-based scan with per-file hash caching.
    ///
    /// Returns the extracted types on success. Any failure — `cargo expand`
    /// crashing, a source file that can't be hashed, a module missing from
    /// the split, a fragment write error — is propagated as an `Err` so the
    /// caller sees the corruption rather than silently falling back to the
    /// raw-source scan. The one silent-skip case is "crate has no `src/`
    /// directory", which we return as an empty vec.
    fn scan_with_expansion_cache(
        &self,
        manifest_dir: &Path,
        crate_name: &str,
    ) -> Result<Vec<EvenframeType>> {
        let src_path = manifest_dir.join("src");
        if !src_path.exists() {
            return Ok(Vec::new());
        }

        // 1. Walk src/ and collect per-file metadata.
        let file_meta = collect_source_files(&src_path, crate_name).map_err(|e| {
            EvenframeError::WorkspaceScan(format!("failed to walk src for '{}': {}", crate_name, e))
        })?;

        if file_meta.is_empty() {
            return Ok(Vec::new());
        }

        // 2. Hash files. Any hash failure is a hard error.
        let hashed: Vec<(SourceFile, String)> = file_meta
            .into_par_iter()
            .map(|meta| {
                let h = expansion_cache::hash_file(&meta.abs_path).map_err(|e| {
                    EvenframeError::WorkspaceScan(format!(
                        "hash failed for {:?}: {}",
                        meta.abs_path, e
                    ))
                })?;
                Ok((meta, h))
            })
            .collect::<Result<Vec<_>>>()?;

        // 3. Load the existing manifest and bucket files by hit/miss.
        let target_dir = expansion_cache::find_target_dir(manifest_dir);
        let cache_dir = expansion_cache::crate_cache_dir(&target_dir, crate_name);
        let manifest = CacheManifest::load(&cache_dir, crate_name);

        let mut hits: Vec<(SourceFile, String, CacheEntry)> = Vec::new();
        let mut misses: Vec<(SourceFile, String)> = Vec::new();
        for (meta, hash) in hashed {
            match manifest.entries.get(&meta.rel_path) {
                Some(entry) if entry.input_hash == hash => {
                    hits.push((meta, hash, entry.clone()));
                }
                _ => misses.push((meta, hash)),
            }
        }

        debug!(
            "[{}] expansion cache: {} hits, {} misses",
            crate_name,
            hits.len(),
            misses.len()
        );

        // 4. Decide expansion strategy and produce new entries. Any failure
        //    here is a hard error — we do NOT fall back to raw scanning, as
        //    that would silently mask corrupt state.
        let new_entries: HashMap<String, CacheEntry> = if misses.is_empty() {
            HashMap::new()
        } else if misses.len() >= CRATE_LEVEL_THRESHOLD {
            self.expand_whole_crate_and_split(manifest_dir, crate_name, &cache_dir, &misses)?
        } else {
            self.expand_per_file(manifest_dir, crate_name, &cache_dir, &misses)?
        };

        // 5. Assemble the output: cache hits + freshly-expanded entries.
        let mut all_types: Vec<EvenframeType> = Vec::new();
        let mut next_manifest = CacheManifest::empty(crate_name);
        for (meta, hash, entry) in hits {
            all_types.extend(entry.extracted_types.iter().cloned());
            next_manifest.entries.insert(
                meta.rel_path,
                CacheEntry {
                    input_hash: hash,
                    ..entry
                },
            );
        }
        for (rel_path, entry) in new_entries {
            all_types.extend(entry.extracted_types.iter().cloned());
            next_manifest.entries.insert(rel_path, entry);
        }

        next_manifest.save(&cache_dir).map_err(|e| {
            EvenframeError::WorkspaceScan(format!(
                "failed to save expansion manifest for '{}': {}",
                crate_name, e
            ))
        })?;

        Ok(all_types)
    }

    /// Runs a single crate-level `cargo expand`, splits it per-module, and
    /// builds `CacheEntry`s for every file in `misses`.
    ///
    /// Any inconsistency between the filesystem layout and the expanded
    /// output — a missing module, an empty fragment, or a write failure —
    /// is a hard error. We do NOT silently fall back to empty fragments:
    /// a 0-byte fragment that gets cached poisons subsequent runs.
    fn expand_whole_crate_and_split(
        &self,
        manifest_dir: &Path,
        crate_name: &str,
        cache_dir: &Path,
        misses: &[(SourceFile, String)],
    ) -> Result<HashMap<String, CacheEntry>> {
        let expanded =
            expansion_cache::expand_crate_full(manifest_dir, crate_name).ok_or_else(|| {
                EvenframeError::WorkspaceScan(format!(
                    "cargo expand failed for crate '{}'. Expansion cache is unusable; \
                     delete the `.evenframe-expanded/` directory and re-run, or disable \
                     `expand_macros` in evenframe.toml.",
                    crate_name
                ))
            })?;
        let parsed = parse_file(&expanded)
            .map_err(|e| EvenframeError::parse_error(Path::new("<expanded>"), e.to_string()))?;
        let by_module = expansion_cache::split_expanded_by_module(&parsed, crate_name);

        let results: Vec<Result<(String, CacheEntry)>> = misses
            .par_iter()
            .map(|(meta, hash)| {
                let source = by_module.get(&meta.module_path).cloned().ok_or_else(|| {
                    EvenframeError::WorkspaceScan(format!(
                        "Expansion cache corrupted: module '{}' (from file {}) was not present \
                         in `cargo expand` output for crate '{}'. The `.evenframe-expanded/` \
                         cache is unrecoverable — delete it and re-run, or disable \
                         `expand_macros` in evenframe.toml.",
                        meta.module_path, meta.rel_path, crate_name
                    ))
                })?;
                let rel_fragment =
                    expansion_cache::write_fragment(cache_dir, &meta.rel_path, &source)?;
                let fragment_abs = cache_dir.join(&rel_fragment);
                let file_path = fragment_abs.to_string_lossy().to_string();
                let extracted = self.extract_from_source(&source, &meta.module_path, &file_path);
                Ok((
                    meta.rel_path.clone(),
                    CacheEntry {
                        input_hash: hash.clone(),
                        module_path: meta.module_path.clone(),
                        fragment_path: rel_fragment,
                        extracted_types: extracted,
                    },
                ))
            })
            .collect();

        let mut entries: HashMap<String, CacheEntry> = HashMap::new();
        for res in results {
            let (rel_path, entry) = res?;
            entries.insert(rel_path, entry);
        }
        Ok(entries)
    }

    /// Runs `cargo expand crate::path` for each miss in a bounded parallel
    /// pool, writes each fragment, and builds `CacheEntry`s.
    fn expand_per_file(
        &self,
        manifest_dir: &Path,
        crate_name: &str,
        cache_dir: &Path,
        misses: &[(SourceFile, String)],
    ) -> Result<HashMap<String, CacheEntry>> {
        // Bound cargo parallelism — every invocation contends on target/.
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4))
            .unwrap_or(2);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .map_err(|e| EvenframeError::Config(format!("rayon pool build: {}", e)))?;

        let expanded_sources: Vec<(SourceFile, String, Option<String>)> = pool.install(|| {
            misses
                .par_iter()
                .map(|(meta, hash)| {
                    let src =
                        expansion_cache::expand_file(manifest_dir, crate_name, &meta.module_path);
                    (meta.clone(), hash.clone(), src)
                })
                .collect()
        });

        let mut entries: HashMap<String, CacheEntry> = HashMap::new();
        for (meta, hash, src_opt) in expanded_sources {
            let Some(source) = src_opt else {
                // One file failed to expand — surface the error so the caller
                // can fall back to raw scanning for the whole crate.
                return Err(EvenframeError::Config(format!(
                    "cargo expand {}::{} failed",
                    crate_name, meta.module_path
                )));
            };
            let rel_fragment = expansion_cache::write_fragment(cache_dir, &meta.rel_path, &source)?;
            let fragment_abs = cache_dir.join(&rel_fragment);
            let file_path = fragment_abs.to_string_lossy().to_string();
            let extracted = self.extract_from_source(&source, &meta.module_path, &file_path);
            entries.insert(
                meta.rel_path.clone(),
                CacheEntry {
                    input_hash: hash,
                    module_path: meta.module_path,
                    fragment_path: rel_fragment,
                    extracted_types: extracted,
                },
            );
        }
        Ok(entries)
    }

    /// Parses an expanded source string and extracts Evenframe types. Used
    /// by both the per-file and crate-level expansion paths. Applies the
    /// same manual-impl merge as the raw-source path.
    fn extract_from_source(
        &self,
        source: &str,
        module_path: &str,
        file_path: &str,
    ) -> Vec<EvenframeType> {
        let syntax_tree = match parse_file(source) {
            Ok(s) => s,
            Err(e) => {
                warn!("failed to parse fragment for {}: {}", module_path, e);
                return Vec::new();
            }
        };
        let mut state = CrateScanState::default();
        self.scan_items_recursive(&syntax_tree.items, &mut state, module_path, file_path);
        state.finalize()
    }

    /// Recursively scans a directory for Rust source files. Test-only
    /// wrapper that produces a finalized `Vec<EvenframeType>` for
    /// single-directory use. Production code uses
    /// [`Self::scan_directory_into`] directly so that manual impls in one
    /// file can resolve against structs in another.
    #[cfg(test)]
    fn scan_directory(
        &self,
        dir: &Path,
        types: &mut Vec<EvenframeType>,
        base_module: &str,
        depth: usize,
    ) -> Result<()> {
        let mut state = CrateScanState::default();
        self.scan_directory_into(dir, &mut state, base_module, depth)?;
        types.extend(state.finalize());
        Ok(())
    }

    /// Recursively scans a directory into a shared [`CrateScanState`], so
    /// that manual trait impls discovered in one file can be merged with
    /// struct definitions in another.
    fn scan_directory_into(
        &self,
        dir: &Path,
        state: &mut CrateScanState,
        base_module: &str,
        depth: usize,
    ) -> Result<()> {
        trace!(
            "Scanning directory: {:?}, module: {}, depth: {}",
            dir, base_module, depth
        );

        if depth > 10 {
            return Err(EvenframeError::MaxRecursionDepth {
                depth: 10,
                path: dir.to_path_buf(),
            });
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.symlink_metadata()?.file_type().is_symlink() {
                debug!("Skipping symlink: {:?}", path);
                continue;
            }

            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if dir_name != "tests" && dir_name != "benches" {
                    let module_path = format!("{}::{}", base_module, dir_name);
                    self.scan_directory_into(&path, state, &module_path, depth + 1)?;
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let file_stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");

                // FIX: Correctly handle `mod.rs` files.
                if file_stem == "lib" || file_stem == "main" {
                    // Crate root, use the base module path directly.
                    self.scan_rust_file_into(&path, state, base_module)?;
                } else if path.file_name().and_then(|n| n.to_str()) == Some("mod.rs") {
                    // A `mod.rs` file defines the module for its parent directory.
                    // The `base_module` path is already correct for this case.
                    self.scan_rust_file_into(&path, state, base_module)?;
                } else {
                    // A regular submodule file (e.g., `user.rs`).
                    let module_path = format!("{}::{}", base_module, file_stem);
                    self.scan_rust_file_into(&path, state, &module_path)?;
                }
            }
        }
        Ok(())
    }

    /// Test-only wrapper around [`Self::scan_rust_file_into`] that
    /// finalizes immediately. Cross-file manual impls are NOT resolved
    /// through this path — use the crate-level scanner for that.
    #[cfg(test)]
    fn scan_rust_file(
        &self,
        path: &Path,
        types: &mut Vec<EvenframeType>,
        module_path: &str,
    ) -> Result<()> {
        let mut state = CrateScanState::default();
        self.scan_rust_file_into(path, &mut state, module_path)?;
        types.extend(state.finalize());
        Ok(())
    }

    /// Parses a single Rust file and accumulates its contents into the
    /// given [`CrateScanState`]. Both structs/enums AND manual trait impls
    /// are collected so the caller can resolve them together.
    fn scan_rust_file_into(
        &self,
        path: &Path,
        state: &mut CrateScanState,
        module_path: &str,
    ) -> Result<()> {
        trace!("Scanning file: {:?}, module: {}", path, module_path);
        let content = fs::read_to_string(path)?;
        let syntax_tree =
            parse_file(&content).map_err(|e| EvenframeError::parse_error(path, e.to_string()))?;

        let file_path = path.to_string_lossy().to_string();
        self.scan_items_recursive(&syntax_tree.items, state, module_path, &file_path);
        Ok(())
    }

    /// Recursively walks syn Items, descending into `mod { ... }` blocks,
    /// tracking module paths, and populating a [`CrateScanState`].
    ///
    /// Handles:
    /// - `Item::Struct` / `Item::Enum` → pending type candidates
    /// - `Item::Impl` → manual trait impl detection (see [`detect_manual_impl`])
    /// - `Item::Mod` with inline content → recurse
    fn scan_items_recursive(
        &self,
        items: &[Item],
        state: &mut CrateScanState,
        module_path: &str,
        file_path: &str,
    ) {
        for item in items {
            match item {
                Item::Struct(s) => {
                    let local_pipeline = detect_pipeline(&s.attrs).or_else(|| {
                        if self.has_apply_alias(&s.attrs) {
                            Some(crate::types::Pipeline::Both)
                        } else {
                            None
                        }
                    });
                    let name = s.ident.to_string();
                    let has_id = has_id_field(&s.fields);
                    trace!(
                        "Collected struct candidate '{}' in '{}' (local_pipeline={:?})",
                        name, module_path, local_pipeline
                    );
                    state.pending.push(PendingType {
                        ident: name,
                        file_path: file_path.to_string(),
                        module_path: module_path.to_string(),
                        kind: TypeKind::Struct,
                        has_id_field: has_id,
                        local_pipeline,
                    });
                }
                Item::Enum(e) => {
                    let local_pipeline = detect_pipeline(&e.attrs).or_else(|| {
                        if self.has_apply_alias(&e.attrs) {
                            Some(crate::types::Pipeline::Both)
                        } else {
                            None
                        }
                    });
                    let name = e.ident.to_string();
                    trace!(
                        "Collected enum candidate '{}' in '{}' (local_pipeline={:?})",
                        name, module_path, local_pipeline
                    );
                    state.pending.push(PendingType {
                        ident: name,
                        file_path: file_path.to_string(),
                        module_path: module_path.to_string(),
                        kind: TypeKind::Enum,
                        has_id_field: false,
                        local_pipeline,
                    });
                }
                Item::Impl(item_impl) => {
                    if let Some((ident, pipeline)) = detect_manual_impl(item_impl) {
                        debug!(
                            "Found manual Evenframe impl for '{}' in module '{}' (pipeline={:?})",
                            ident, module_path, pipeline
                        );
                        // If the same type has multiple manual impls, keep
                        // the strongest pipeline (Both > Typesync/Schemasync).
                        state
                            .manual_impls
                            .entry(ident)
                            .and_modify(|existing| {
                                if matches!(pipeline, crate::types::Pipeline::Both) {
                                    *existing = pipeline;
                                }
                            })
                            .or_insert(pipeline);
                    }
                }
                Item::Mod(m) => {
                    if let Some((_, mod_items)) = &m.content {
                        let child_module = format!("{}::{}", module_path, m.ident);
                        self.scan_items_recursive(mod_items, state, &child_module, file_path);
                    }
                }
                _ => {}
            }
        }
    }

    /// Checks for `#[apply(Alias)]` attributes.
    fn has_apply_alias(&self, attrs: &[Attribute]) -> bool {
        self.apply_aliases.iter().any(|alias| {
            attrs.iter().any(|attr| {
                if attr.path().is_ident("apply")
                    && let Meta::List(meta_list) = &attr.meta
                {
                    return meta_list.tokens.to_string() == *alias;
                }

                false
            })
        })
    }
}

/// A source file discovered while walking `src/`, paired with its module
/// path (so it can be passed to `cargo expand crate::module::path`) and a
/// path relative to the crate's `src/` directory (so it can be used as a
/// cache manifest key).
#[derive(Debug, Clone)]
struct SourceFile {
    abs_path: PathBuf,
    /// Path relative to the crate's `src/` directory, using forward slashes
    /// regardless of platform. E.g. `lib.rs`, `foo.rs`, `bar/baz.rs`.
    rel_path: String,
    /// Fully-qualified module path including the crate name prefix.
    /// E.g. `my_crate`, `my_crate::foo`, `my_crate::bar::baz`.
    module_path: String,
}

/// Walks a crate's `src/` directory and returns metadata for every `.rs`
/// file discovered. Mirrors the module-path resolution rules of
/// [`WorkspaceScanner::scan_directory_into`] without doing any parsing.
fn collect_source_files(src_path: &Path, crate_name: &str) -> Result<Vec<SourceFile>> {
    let mut out = Vec::new();
    walk_src(src_path, crate_name, "", crate_name, &mut out, 0)?;
    Ok(out)
}

fn walk_src(
    dir: &Path,
    base_module: &str,
    rel_dir: &str,
    _crate_name: &str,
    out: &mut Vec<SourceFile>,
    depth: usize,
) -> Result<()> {
    if depth > 10 {
        return Err(EvenframeError::MaxRecursionDepth {
            depth: 10,
            path: dir.to_path_buf(),
        });
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.symlink_metadata()?.file_type().is_symlink() {
            continue;
        }

        if path.is_dir() {
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // `tests/` and `benches/` are separate cargo targets that don't
            // show up in `cargo expand --lib`. `bin/` is similar — each file
            // is an extra binary, not part of the library.
            if dir_name == "tests" || dir_name == "benches" || dir_name == "bin" {
                continue;
            }
            let child_module = format!("{}::{}", base_module, dir_name);
            let child_rel = if rel_dir.is_empty() {
                dir_name.to_string()
            } else {
                format!("{}/{}", rel_dir, dir_name)
            };
            walk_src(
                &path,
                &child_module,
                &child_rel,
                _crate_name,
                out,
                depth + 1,
            )?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let file_stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            // `src/main.rs` is the default binary target. It doesn't appear
            // in `cargo expand --lib` output, so including it would always
            // produce a missing-module error in the expansion path.
            if file_stem == "main" {
                continue;
            }
            let rel_path = if rel_dir.is_empty() {
                file_name.to_string()
            } else {
                format!("{}/{}", rel_dir, file_name)
            };
            let module_path = if file_stem == "lib" || file_name == "mod.rs" {
                base_module.to_string()
            } else {
                format!("{}::{}", base_module, file_stem)
            };
            out.push(SourceFile {
                abs_path: path,
                rel_path,
                module_path,
            });
        }
    }
    Ok(())
}

/// If `item` is a trait impl for one of the Evenframe traits on a bare
/// type ident, returns `(self_ty_ident, pipeline)`. Otherwise returns
/// `None`.
///
/// Skipped:
/// - impls carrying `#[automatically_derived]` (these come from derive
///   macro expansion and must not be mistaken for user code);
/// - impls whose `Self` type is not a bare ident (no paths like
///   `other_crate::T`, no generics like `Wrapper<T>`);
/// - impls of traits other than
///   `EvenframePersistableStruct` / `EvenframeAppStruct` / `EvenframeTaggedUnion`
///   (`EvenframeDeserialize` is supplementary and does not opt a type in on
///   its own).
fn detect_manual_impl(item: &ItemImpl) -> Option<(String, crate::types::Pipeline)> {
    if item
        .attrs
        .iter()
        .any(|a| a.path().is_ident("automatically_derived"))
    {
        return None;
    }

    let (_, trait_path, _) = item.trait_.as_ref()?;
    let trait_name = trait_path.segments.last()?.ident.to_string();
    let pipeline = match trait_name.as_str() {
        "EvenframePersistableStruct" | "EvenframeAppStruct" | "EvenframeTaggedUnion" => {
            crate::types::Pipeline::Both
        }
        _ => return None,
    };

    // Self type must be a bare ident (no path, no generics).
    let self_ty_ident = match item.self_ty.as_ref() {
        syn::Type::Path(tp) if tp.qself.is_none() && tp.path.segments.len() == 1 => {
            let seg = &tp.path.segments[0];
            if !matches!(seg.arguments, syn::PathArguments::None) {
                trace!(
                    "Skipping manual impl of '{}' — self type has generic args",
                    trait_name
                );
                return None;
            }
            seg.ident.to_string()
        }
        _ => {
            trace!(
                "Skipping manual impl of '{}' — self type is not a bare ident",
                trait_name
            );
            return None;
        }
    };

    Some((self_ty_ident, pipeline))
}

/// Detects which derive macro is present and returns the corresponding Pipeline.
/// Returns None if no relevant derive is found.
fn detect_pipeline(attrs: &[Attribute]) -> Option<crate::types::Pipeline> {
    use crate::types::Pipeline;

    let mut has_typesync = false;
    let mut has_schemasync = false;
    let mut has_evenframe = false;

    for attr in attrs {
        if attr.path().is_ident("derive")
            && let Meta::List(meta_list) = &attr.meta
        {
            let tokens_str = meta_list.tokens.to_string();
            if tokens_str.contains("Typesync") {
                has_typesync = true;
            }
            if tokens_str.contains("Schemasync") {
                has_schemasync = true;
            }
            if tokens_str.contains("Evenframe") {
                has_evenframe = true;
            }
        }
    }

    if has_evenframe || (has_typesync && has_schemasync) {
        Some(Pipeline::Both)
    } else if has_typesync {
        Some(Pipeline::Typesync)
    } else if has_schemasync {
        Some(Pipeline::Schemasync)
    } else {
        None
    }
}

/// Checks if a struct has a field named `id`.
fn has_id_field(fields: &syn::Fields) -> bool {
    if let syn::Fields::Named(fields_named) = fields {
        fields_named
            .named
            .iter()
            .any(|field| field.ident.as_ref().is_some_and(|id| id == "id"))
    } else {
        false
    }
}

/// Helper function to extract unique module paths from the found types.
pub fn get_unique_modules(types: &[EvenframeType]) -> Vec<String> {
    let mut modules: HashSet<_> = types.iter().map(|t| t.module_path.clone()).collect();
    let unique_modules: Vec<String> = modules.drain().collect();
    debug!(
        "Found {} unique modules from {} types",
        unique_modules.len(),
        types.len()
    );
    trace!("Unique modules: {:?}", unique_modules);
    unique_modules
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    // ==================== TypeKind Tests ====================

    #[test]
    fn test_type_kind_equality() {
        assert_eq!(TypeKind::Struct, TypeKind::Struct);
        assert_eq!(TypeKind::Enum, TypeKind::Enum);
        assert_ne!(TypeKind::Struct, TypeKind::Enum);
    }

    #[test]
    fn test_type_kind_debug() {
        assert_eq!(format!("{:?}", TypeKind::Struct), "Struct");
        assert_eq!(format!("{:?}", TypeKind::Enum), "Enum");
    }

    #[test]
    fn test_type_kind_clone() {
        let kind = TypeKind::Struct;
        let cloned = kind.clone();
        assert_eq!(kind, cloned);
    }

    // ==================== EvenframeType Tests ====================

    #[test]
    fn test_evenframe_type_creation() {
        let ef_type = EvenframeType {
            name: "User".to_string(),
            module_path: "my_crate::models".to_string(),
            file_path: "/path/to/file.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: true,
            pipeline: crate::types::Pipeline::Both,
        };

        assert_eq!(ef_type.name, "User");
        assert_eq!(ef_type.module_path, "my_crate::models");
        assert_eq!(ef_type.file_path, "/path/to/file.rs");
        assert_eq!(ef_type.kind, TypeKind::Struct);
        assert!(ef_type.has_id_field);
    }

    #[test]
    fn test_evenframe_type_qualified_name() {
        let ef_type = EvenframeType {
            name: "User".to_string(),
            module_path: "my_crate::models".to_string(),
            file_path: "/path/to/file.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: true,
            pipeline: crate::types::Pipeline::Both,
        };

        assert_eq!(ef_type.qualified_name(), "my_crate::models::User");
    }

    #[test]
    fn test_evenframe_type_display() {
        let ef_type = EvenframeType {
            name: "User".to_string(),
            module_path: "my_crate::models".to_string(),
            file_path: "/path/to/file.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: true,
            pipeline: crate::types::Pipeline::Both,
        };

        assert_eq!(format!("{}", ef_type), "my_crate::models::User");
    }

    #[test]
    fn test_evenframe_type_clone() {
        let ef_type = EvenframeType {
            name: "Order".to_string(),
            module_path: "crate::orders".to_string(),
            file_path: "/orders.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: false,
            pipeline: crate::types::Pipeline::Both,
        };

        let cloned = ef_type.clone();
        assert_eq!(ef_type.name, cloned.name);
        assert_eq!(ef_type.module_path, cloned.module_path);
        assert_eq!(ef_type.file_path, cloned.file_path);
        assert_eq!(ef_type.kind, cloned.kind);
        assert_eq!(ef_type.has_id_field, cloned.has_id_field);
    }

    #[test]
    fn test_evenframe_type_debug() {
        let ef_type = EvenframeType {
            name: "Test".to_string(),
            module_path: "crate".to_string(),
            file_path: "/test.rs".to_string(),
            kind: TypeKind::Enum,
            has_id_field: false,
            pipeline: crate::types::Pipeline::Both,
        };

        let debug_str = format!("{:?}", ef_type);
        assert!(debug_str.contains("Test"));
        assert!(debug_str.contains("Enum"));
    }

    // ==================== WorkspaceScanner Tests ====================

    #[test]
    fn test_workspace_scanner_with_path() {
        let path = PathBuf::from("/some/path");
        let aliases = vec!["MyMacro".to_string()];
        let scanner = WorkspaceScanner::with_path(path.clone(), aliases.clone(), false);

        assert_eq!(scanner.start_path, path);
        assert_eq!(scanner.apply_aliases, aliases);
    }

    #[test]
    fn test_workspace_scanner_with_empty_aliases() {
        let path = PathBuf::from("/test");
        let scanner = WorkspaceScanner::with_path(path, vec![], false);

        assert!(scanner.apply_aliases.is_empty());
    }

    #[test]
    fn test_workspace_scanner_with_multiple_aliases() {
        let path = PathBuf::from("/test");
        let aliases = vec![
            "Macro1".to_string(),
            "Macro2".to_string(),
            "Macro3".to_string(),
        ];
        let scanner = WorkspaceScanner::with_path(path, aliases.clone(), false);

        assert_eq!(scanner.apply_aliases.len(), 3);
        assert!(scanner.apply_aliases.contains(&"Macro1".to_string()));
        assert!(scanner.apply_aliases.contains(&"Macro2".to_string()));
        assert!(scanner.apply_aliases.contains(&"Macro3".to_string()));
    }

    // ==================== detect_pipeline Tests ====================

    #[test]
    fn test_detect_pipeline_with_derive_evenframe() {
        let code = r#"
            #[derive(Debug, Clone, Evenframe)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert_eq!(
                detect_pipeline(&s.attrs),
                Some(crate::types::Pipeline::Both)
            );
        }
    }

    #[test]
    fn test_detect_pipeline_without_evenframe() {
        let code = r#"
            #[derive(Debug, Clone)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert_eq!(detect_pipeline(&s.attrs), None);
        }
    }

    #[test]
    fn test_detect_pipeline_with_no_derive_attr() {
        let code = r#"
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert_eq!(detect_pipeline(&s.attrs), None);
        }
    }

    #[test]
    fn test_detect_pipeline_only_evenframe() {
        let code = r#"
            #[derive(Evenframe)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert_eq!(
                detect_pipeline(&s.attrs),
                Some(crate::types::Pipeline::Both)
            );
        }
    }

    #[test]
    fn test_detect_pipeline_typesync_only() {
        let code = r#"
            #[derive(Typesync)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert_eq!(
                detect_pipeline(&s.attrs),
                Some(crate::types::Pipeline::Typesync)
            );
        }
    }

    #[test]
    fn test_detect_pipeline_schemasync_only() {
        let code = r#"
            #[derive(Schemasync)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert_eq!(
                detect_pipeline(&s.attrs),
                Some(crate::types::Pipeline::Schemasync)
            );
        }
    }

    #[test]
    fn test_detect_pipeline_both_typesync_and_schemasync() {
        let code = r#"
            #[derive(Typesync, Schemasync)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert_eq!(
                detect_pipeline(&s.attrs),
                Some(crate::types::Pipeline::Both)
            );
        }
    }

    // ==================== has_id_field Tests ====================

    #[test]
    fn test_has_id_field_with_id() {
        let code = r#"
            struct TestStruct {
                id: String,
                name: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_without_id() {
        let code = r#"
            struct TestStruct {
                name: String,
                age: i32,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(!has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_tuple_struct() {
        let code = r#"
            struct TestStruct(String, i32);
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            // Tuple structs have unnamed fields, should return false
            assert!(!has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_unit_struct() {
        let code = r#"
            struct TestStruct;
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(!has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_only_id() {
        let code = r#"
            struct TestStruct {
                id: i64,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(has_id_field(&s.fields));
        }
    }

    // ==================== get_unique_modules Tests ====================

    #[test]
    fn test_get_unique_modules_empty() {
        let types: Vec<EvenframeType> = vec![];
        let modules = get_unique_modules(&types);
        assert!(modules.is_empty());
    }

    #[test]
    fn test_get_unique_modules_single() {
        let types = vec![EvenframeType {
            name: "User".to_string(),
            module_path: "crate::models".to_string(),
            file_path: "/path.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: true,
            pipeline: crate::types::Pipeline::Both,
        }];

        let modules = get_unique_modules(&types);
        assert_eq!(modules.len(), 1);
        assert!(modules.contains(&"crate::models".to_string()));
    }

    #[test]
    fn test_get_unique_modules_duplicates() {
        let types = vec![
            EvenframeType {
                name: "User".to_string(),
                module_path: "crate::models".to_string(),
                file_path: "/path1.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
                pipeline: crate::types::Pipeline::Both,
            },
            EvenframeType {
                name: "Order".to_string(),
                module_path: "crate::models".to_string(),
                file_path: "/path2.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
                pipeline: crate::types::Pipeline::Both,
            },
        ];

        let modules = get_unique_modules(&types);
        assert_eq!(modules.len(), 1);
        assert!(modules.contains(&"crate::models".to_string()));
    }

    #[test]
    fn test_get_unique_modules_different_modules() {
        let types = vec![
            EvenframeType {
                name: "User".to_string(),
                module_path: "crate::models::user".to_string(),
                file_path: "/path1.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
                pipeline: crate::types::Pipeline::Both,
            },
            EvenframeType {
                name: "Order".to_string(),
                module_path: "crate::models::order".to_string(),
                file_path: "/path2.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
                pipeline: crate::types::Pipeline::Both,
            },
            EvenframeType {
                name: "Status".to_string(),
                module_path: "crate::enums".to_string(),
                file_path: "/path3.rs".to_string(),
                kind: TypeKind::Enum,
                has_id_field: false,
                pipeline: crate::types::Pipeline::Both,
            },
        ];

        let modules = get_unique_modules(&types);
        assert_eq!(modules.len(), 3);
    }

    // ==================== Filesystem-Based Tests ====================

    fn create_rust_file(dir: &Path, filename: &str, content: &str) -> std::io::Result<()> {
        let file_path = dir.join(filename);
        let mut file = File::create(file_path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    #[test]
    fn test_scan_rust_file_finds_evenframe_struct() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Debug, Clone, Evenframe)]
            pub struct User {
                pub id: String,
                pub name: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "user.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("user.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
        assert_eq!(types[0].module_path, "test_crate::models");
        assert_eq!(types[0].kind, TypeKind::Struct);
        assert!(types[0].has_id_field);
    }

    #[test]
    fn test_scan_rust_file_finds_evenframe_enum() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Debug, Clone, Evenframe)]
            pub enum Status {
                Active,
                Inactive,
                Pending,
            }
        "#;

        create_rust_file(temp_dir.path(), "status.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("status.rs"),
                &mut types,
                "test_crate::enums",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "Status");
        assert_eq!(types[0].module_path, "test_crate::enums");
        assert_eq!(types[0].kind, TypeKind::Enum);
        assert!(!types[0].has_id_field);
    }

    #[test]
    fn test_scan_rust_file_ignores_non_evenframe_types() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Debug, Clone)]
            pub struct RegularStruct {
                pub name: String,
            }

            pub enum RegularEnum {
                A,
                B,
            }
        "#;

        create_rust_file(temp_dir.path(), "regular.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("regular.rs"),
                &mut types,
                "test_crate",
            )
            .unwrap();

        assert!(types.is_empty());
    }

    #[test]
    fn test_scan_rust_file_finds_multiple_types() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
                pub name: String,
            }

            #[derive(Evenframe)]
            pub struct Order {
                pub id: String,
                pub total: f64,
            }

            #[derive(Evenframe)]
            pub enum Status {
                Active,
                Inactive,
            }
        "#;

        create_rust_file(temp_dir.path(), "models.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("models.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert_eq!(types.len(), 3);

        let names: Vec<_> = types.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"User"));
        assert!(names.contains(&"Order"));
        assert!(names.contains(&"Status"));
    }

    #[test]
    fn test_scan_rust_file_with_apply_alias() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[apply(MyMacro)]
            pub struct User {
                pub id: String,
                pub name: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "user.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(
            temp_dir.path().to_path_buf(),
            vec!["MyMacro".to_string()],
            false,
        );
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("user.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
    }

    #[test]
    fn test_scan_rust_file_without_matching_apply_alias() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[apply(OtherMacro)]
            pub struct User {
                pub id: String,
                pub name: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "user.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(
            temp_dir.path().to_path_buf(),
            vec!["MyMacro".to_string()],
            false,
        );
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("user.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert!(types.is_empty());
    }

    #[test]
    fn test_scan_directory_with_src_layout() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();

        let content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_directory(&src_dir, &mut types, "test_crate", 0)
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
        assert_eq!(types[0].module_path, "test_crate");
    }

    #[test]
    fn test_scan_directory_skips_tests_directory() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        let tests_dir = src_dir.join("tests");

        fs::create_dir_all(&tests_dir).unwrap();

        let main_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        let test_content = r#"
            #[derive(Evenframe)]
            pub struct TestType {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", main_content).unwrap();
        create_rust_file(&tests_dir, "test.rs", test_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_directory(&src_dir, &mut types, "test_crate", 0)
            .unwrap();

        // Should only find the main type, not the test type
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
    }

    #[test]
    fn test_scan_directory_skips_benches_directory() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        let benches_dir = src_dir.join("benches");

        fs::create_dir_all(&benches_dir).unwrap();

        let main_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        let bench_content = r#"
            #[derive(Evenframe)]
            pub struct BenchType {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", main_content).unwrap();
        create_rust_file(&benches_dir, "bench.rs", bench_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_directory(&src_dir, &mut types, "test_crate", 0)
            .unwrap();

        // Should only find the main type, not the bench type
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
    }

    #[test]
    fn test_scan_directory_max_recursion_depth() {
        let temp_dir = TempDir::new().unwrap();
        let mut current_dir = temp_dir.path().to_path_buf();

        // Create a deeply nested directory structure
        for i in 0..12 {
            current_dir = current_dir.join(format!("level_{}", i));
            fs::create_dir(&current_dir).unwrap();
        }

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        let result = scanner.scan_directory(temp_dir.path(), &mut types, "test_crate", 0);

        // Should hit max recursion depth and return an error
        assert!(result.is_err());
        if let Err(EvenframeError::MaxRecursionDepth { depth, .. }) = result {
            assert_eq!(depth, 10);
        } else {
            panic!("Expected MaxRecursionDepth error");
        }
    }

    #[test]
    fn test_scan_directory_handles_mod_rs() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        let models_dir = src_dir.join("models");

        fs::create_dir_all(&models_dir).unwrap();

        let mod_content = r#"
            #[derive(Evenframe)]
            pub struct ModUser {
                pub id: String,
            }
        "#;

        create_rust_file(&models_dir, "mod.rs", mod_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_directory(&src_dir, &mut types, "test_crate", 0)
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "ModUser");
        // mod.rs should use the parent directory's module path
        assert_eq!(types[0].module_path, "test_crate::models");
    }

    #[test]
    fn test_scan_directory_handles_submodule_files() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");

        fs::create_dir(&src_dir).unwrap();

        let user_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        let order_content = r#"
            #[derive(Evenframe)]
            pub struct Order {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", "").unwrap();
        create_rust_file(&src_dir, "user.rs", user_content).unwrap();
        create_rust_file(&src_dir, "order.rs", order_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_directory(&src_dir, &mut types, "test_crate", 0)
            .unwrap();

        assert_eq!(types.len(), 2);

        // Check module paths are correct for submodule files
        let user_type = types.iter().find(|t| t.name == "User").unwrap();
        let order_type = types.iter().find(|t| t.name == "Order").unwrap();

        assert_eq!(user_type.module_path, "test_crate::user");
        assert_eq!(order_type.module_path, "test_crate::order");
    }

    #[test]
    fn test_process_manifest_single_crate() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();

        let cargo_toml = r#"
            [package]
            name = "my_crate"
            version = "0.1.0"
            edition = "2024"
        "#;

        let lib_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "Cargo.toml", cargo_toml).unwrap();
        create_rust_file(&src_dir, "lib.rs", lib_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);

        let types = scanner
            .process_manifest(&temp_dir.path().join("Cargo.toml"))
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
    }

    #[test]
    fn test_scan_for_evenframe_types_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);

        let types = scanner.scan_for_evenframe_types().unwrap();

        assert!(types.is_empty());
    }

    #[test]
    fn test_scan_rust_file_struct_without_id() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Evenframe)]
            pub struct Address {
                pub street: String,
                pub city: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "address.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("address.rs"),
                &mut types,
                "test_crate",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "Address");
        assert!(!types[0].has_id_field);
    }

    // ==================== detect_manual_impl Tests ====================

    fn parse_item_impl(code: &str) -> syn::ItemImpl {
        let file = syn::parse_file(code).unwrap();
        match file.items.into_iter().next().unwrap() {
            syn::Item::Impl(i) => i,
            _ => panic!("expected Item::Impl"),
        }
    }

    #[test]
    fn test_detect_manual_impl_persistable_struct() {
        let item = parse_item_impl(
            r#"
            impl EvenframePersistableStruct for User {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }
            "#,
        );
        let result = detect_manual_impl(&item);
        assert_eq!(
            result,
            Some(("User".to_string(), crate::types::Pipeline::Both))
        );
    }

    #[test]
    fn test_detect_manual_impl_app_struct() {
        let item = parse_item_impl(
            r#"
            impl EvenframeAppStruct for Address {
                fn struct_config() -> StructConfig { unimplemented!() }
            }
            "#,
        );
        let result = detect_manual_impl(&item);
        assert_eq!(
            result,
            Some(("Address".to_string(), crate::types::Pipeline::Both))
        );
    }

    #[test]
    fn test_detect_manual_impl_tagged_union() {
        let item = parse_item_impl(
            r#"
            impl EvenframeTaggedUnion for Status {
                fn variants() -> TaggedUnion { unimplemented!() }
            }
            "#,
        );
        let result = detect_manual_impl(&item);
        assert_eq!(
            result,
            Some(("Status".to_string(), crate::types::Pipeline::Both))
        );
    }

    #[test]
    fn test_detect_manual_impl_ignores_deserialize_trait() {
        // EvenframeDeserialize is supplementary and should not opt a type in
        // on its own.
        let item = parse_item_impl(
            r#"
            impl<'de> EvenframeDeserialize<'de> for Foo {
                fn evenframe_deserialize<D>(d: D) -> Result<Self, D::Error>
                where D: Deserializer<'de> { unimplemented!() }
            }
            "#,
        );
        assert_eq!(detect_manual_impl(&item), None);
    }

    #[test]
    fn test_detect_manual_impl_ignores_unrelated_trait() {
        let item = parse_item_impl(
            r#"
            impl Default for Foo {
                fn default() -> Self { unimplemented!() }
            }
            "#,
        );
        assert_eq!(detect_manual_impl(&item), None);
    }

    #[test]
    fn test_detect_manual_impl_ignores_inherent_impl() {
        let item = parse_item_impl(
            r#"
            impl Foo {
                fn thing(&self) {}
            }
            "#,
        );
        assert_eq!(detect_manual_impl(&item), None);
    }

    #[test]
    fn test_detect_manual_impl_skips_automatically_derived() {
        // Derive macro output looks exactly like a manual impl at the
        // token level. The #[automatically_derived] attribute is our only
        // signal.
        let item = parse_item_impl(
            r#"
            #[automatically_derived]
            impl EvenframePersistableStruct for User {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }
            "#,
        );
        assert_eq!(detect_manual_impl(&item), None);
    }

    #[test]
    fn test_detect_manual_impl_skips_generic_self_type() {
        let item = parse_item_impl(
            r#"
            impl<T> EvenframePersistableStruct for Wrapper<T> {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }
            "#,
        );
        assert_eq!(detect_manual_impl(&item), None);
    }

    #[test]
    fn test_detect_manual_impl_accepts_crate_prefixed_trait_path() {
        // We match on the LAST segment of the trait path, so fully-qualified
        // references to the evenframe trait still work.
        let item = parse_item_impl(
            r#"
            impl evenframe_core::traits::EvenframePersistableStruct for User {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }
            "#,
        );
        assert_eq!(
            detect_manual_impl(&item),
            Some(("User".to_string(), crate::types::Pipeline::Both))
        );
    }

    // ==================== Manual impl end-to-end scanner tests ====================

    #[test]
    fn test_scan_rust_file_manual_impl_in_same_file() {
        let temp_dir = TempDir::new().unwrap();
        // No derive — only a manual impl.
        let content = r#"
            pub struct Foo {
                pub id: String,
                pub name: String,
            }

            impl EvenframePersistableStruct for Foo {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }
        "#;

        create_rust_file(temp_dir.path(), "foo.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();
        scanner
            .scan_rust_file(&temp_dir.path().join("foo.rs"), &mut types, "test_crate")
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "Foo");
        assert_eq!(types[0].kind, TypeKind::Struct);
        assert!(types[0].has_id_field);
        assert_eq!(types[0].pipeline, crate::types::Pipeline::Both);
    }

    #[test]
    fn test_scan_rust_file_manual_impl_ignores_automatically_derived() {
        let temp_dir = TempDir::new().unwrap();
        // Simulates derive macro output — no user-facing derive at all, only
        // an automatically_derived impl. Without a derive AND without a
        // manual impl, the struct should NOT be included.
        let content = r#"
            pub struct Foo {
                pub id: String,
            }

            #[automatically_derived]
            impl EvenframePersistableStruct for Foo {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }
        "#;

        create_rust_file(temp_dir.path(), "foo.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();
        scanner
            .scan_rust_file(&temp_dir.path().join("foo.rs"), &mut types, "test_crate")
            .unwrap();

        assert!(types.is_empty());
    }

    #[test]
    fn test_manual_impl_enum_tagged_union() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            pub enum Status {
                Active,
                Inactive,
            }

            impl EvenframeTaggedUnion for Status {
                fn variants() -> TaggedUnion { unimplemented!() }
            }
        "#;

        create_rust_file(temp_dir.path(), "status.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();
        scanner
            .scan_rust_file(&temp_dir.path().join("status.rs"), &mut types, "test_crate")
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "Status");
        assert_eq!(types[0].kind, TypeKind::Enum);
        assert_eq!(types[0].pipeline, crate::types::Pipeline::Both);
    }

    #[test]
    fn test_manual_impl_in_separate_file_via_scan_directory() {
        // The whole point of the two-pass merge: a struct in models.rs and
        // its manual impl in impls.rs should still be linked up.
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();

        create_rust_file(
            &src_dir,
            "lib.rs",
            r#"
                pub mod models;
                pub mod impls;
            "#,
        )
        .unwrap();

        create_rust_file(
            &src_dir,
            "models.rs",
            r#"
                pub struct Foo {
                    pub id: String,
                    pub name: String,
                }
            "#,
        )
        .unwrap();

        create_rust_file(
            &src_dir,
            "impls.rs",
            r#"
                impl EvenframePersistableStruct for Foo {
                    fn static_table_config() -> TableConfig { unimplemented!() }
                }
            "#,
        )
        .unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();
        scanner
            .scan_directory(&src_dir, &mut types, "test_crate", 0)
            .unwrap();

        assert_eq!(types.len(), 1, "expected exactly one type, got {:?}", types);
        assert_eq!(types[0].name, "Foo");
        // file_path should point at the struct definition, not the impl.
        assert!(
            types[0].file_path.ends_with("models.rs"),
            "expected file_path to end with models.rs, got {}",
            types[0].file_path
        );
        assert_eq!(types[0].module_path, "test_crate::models");
        assert!(types[0].has_id_field);
    }

    #[test]
    fn test_manual_impl_multiple_impls_same_type() {
        // A type with impls of multiple evenframe traits should still only
        // produce one EvenframeType entry (keyed by struct ident).
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            pub struct Foo {
                pub id: String,
            }

            impl EvenframePersistableStruct for Foo {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }

            impl EvenframeAppStruct for Foo {
                fn struct_config() -> StructConfig { unimplemented!() }
            }
        "#;

        create_rust_file(temp_dir.path(), "foo.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();
        scanner
            .scan_rust_file(&temp_dir.path().join("foo.rs"), &mut types, "test_crate")
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "Foo");
    }

    #[test]
    fn test_derive_still_wins_over_manual_impl() {
        // When both a derive and a matching manual impl exist, the derive's
        // pipeline is used (the manual impl should be filtered as
        // #[automatically_derived] in real expand output — but in raw source
        // with a derive PLUS a manual impl, derive wins the pipeline choice).
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Typesync)]
            pub struct Foo {
                pub id: String,
            }

            impl EvenframePersistableStruct for Foo {
                fn static_table_config() -> TableConfig { unimplemented!() }
            }
        "#;

        create_rust_file(temp_dir.path(), "foo.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();
        scanner
            .scan_rust_file(&temp_dir.path().join("foo.rs"), &mut types, "test_crate")
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].pipeline, crate::types::Pipeline::Typesync);
    }

    // ==================== collect_source_files Tests ====================

    #[test]
    fn test_collect_source_files_basic_layout() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        let sub_dir = src_dir.join("sub");
        fs::create_dir(&sub_dir).unwrap();

        create_rust_file(&src_dir, "lib.rs", "").unwrap();
        create_rust_file(&src_dir, "foo.rs", "").unwrap();
        create_rust_file(&sub_dir, "bar.rs", "").unwrap();
        create_rust_file(&sub_dir, "mod.rs", "").unwrap();

        let files = collect_source_files(&src_dir, "my_crate").unwrap();
        let by_rel: HashMap<String, String> = files
            .iter()
            .map(|f| (f.rel_path.clone(), f.module_path.clone()))
            .collect();

        assert_eq!(by_rel.get("lib.rs").map(String::as_str), Some("my_crate"));
        assert_eq!(
            by_rel.get("foo.rs").map(String::as_str),
            Some("my_crate::foo")
        );
        assert_eq!(
            by_rel.get("sub/bar.rs").map(String::as_str),
            Some("my_crate::sub::bar")
        );
        assert_eq!(
            by_rel.get("sub/mod.rs").map(String::as_str),
            Some("my_crate::sub")
        );
    }

    #[test]
    fn test_collect_source_files_skips_tests_and_benches() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::create_dir(src_dir.join("tests")).unwrap();
        fs::create_dir(src_dir.join("benches")).unwrap();

        create_rust_file(&src_dir, "lib.rs", "").unwrap();
        create_rust_file(&src_dir.join("tests"), "should_skip.rs", "").unwrap();
        create_rust_file(&src_dir.join("benches"), "should_skip.rs", "").unwrap();

        let files = collect_source_files(&src_dir, "my_crate").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].rel_path, "lib.rs");
    }

    #[test]
    fn test_collect_source_files_skips_main_and_bin() {
        // `cargo expand --lib` does not emit the `main.rs` or `src/bin/*`
        // targets, so the expansion-cache path must not walk them: they'd
        // look like "missing module" errors.
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        fs::create_dir(src_dir.join("bin")).unwrap();

        create_rust_file(&src_dir, "lib.rs", "").unwrap();
        create_rust_file(&src_dir, "main.rs", "").unwrap();
        create_rust_file(&src_dir, "utils.rs", "").unwrap();
        create_rust_file(&src_dir.join("bin"), "extra.rs", "").unwrap();

        let files = collect_source_files(&src_dir, "my_crate").unwrap();
        let mut names: Vec<_> = files.iter().map(|f| f.rel_path.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["lib.rs".to_string(), "utils.rs".to_string()]);
    }
}
