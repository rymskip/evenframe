//! Workspace scanning for finding Rust types with Evenframe derives.

use super::configs::{ParsedType, has_database_id, parse_scanned_item};
use super::paths::{ModuleScope, rust_path};
use crate::config::IncludeFile;
use crate::error::{EvenframeError, Result};
use crate::scan::expansion::{self, CacheEntry, CacheManifest};
use ignore::WalkBuilder;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use syn::{Attribute, Item, ItemEnum, ItemImpl, ItemStruct, Meta, parse_file};
use tracing::{debug, info, trace, warn};

/// How deep below a crate's `src/` the scan descends before failing with
/// [`EvenframeError::MaxRecursionDepth`].
pub const MAX_SCAN_DEPTH: usize = 10;

/// Every `Cargo.toml` under `root`, sorted, skipping anything a
/// `.gitignore` excludes (such as `target/`). Like macroforge's scanner,
/// hidden entries are walked and only the project's own `.gitignore` files
/// apply (not the global, `.git/info/exclude`, or any above `root`, so a project
/// checked out inside another one's ignored directory scans normally); unlike
/// it, they apply even outside a git repository.
///
/// `include_files` entries don't go through this walk: they are read
/// directly, so listing a gitignored path there still scans it.
pub fn find_manifests(root: &Path) -> Vec<PathBuf> {
    let mut manifests: Vec<PathBuf> = WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_name() == "Cargo.toml" && entry.file_type().is_some_and(|t| t.is_file())
        })
        .map(|entry| entry.into_path())
        .collect();
    manifests.sort();
    manifests
}

/// The canonical paths of `manifests`, for telling whether a workspace
/// member has its own manifest in the scan.
pub fn canonical_manifests(manifests: &[PathBuf]) -> HashSet<PathBuf> {
    manifests
        .iter()
        .filter_map(|manifest| fs::canonicalize(manifest).ok())
        .collect()
}

/// Whether the workspace member at `member_dir` is itself one of the scanned
/// manifests, so it is scanned as its own package rather than as a member.
pub fn member_has_own_manifest(member_dir: &Path, known: &HashSet<PathBuf>) -> bool {
    fs::canonicalize(member_dir.join("Cargo.toml")).is_ok_and(|manifest| known.contains(&manifest))
}

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
    /// Whether this struct has a field the database stores as `id` (makes it
    /// a table).
    pub has_id_field: bool,
    /// Which pipeline(s) this type participates in.
    pub pipeline: crate::types::Pipeline,
    /// When true, this type is registered for field-type resolution only and is
    /// skipped at every emission site (schemasync `DEFINE TABLE`/mock/diff,
    /// typesync interface output). Set for types from a `resolve_only`
    /// `include_files` entry. Defaults to false for normal scanned types.
    #[serde(default)]
    pub resolve_only: bool,
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

/// The syntax of a scanned struct or enum, kept until its crate's manual
/// impls are known and its configuration can be parsed.
pub(super) enum ScannedAst {
    Struct(ItemStruct),
    Enum(ItemEnum),
}

/// A type the scan found, with its configuration parsed from its syntax.
/// A parse failure is kept with the type so the build reports it in file
/// order.
#[derive(Debug)]
pub struct ScannedItem {
    pub evenframe_type: EvenframeType,
    pub parsed: Result<ParsedType>,
}

/// What a scan found: every Evenframe type, and the scope of every module it
/// parsed, by module path, for resolving the types fields name.
#[derive(Debug, Default)]
pub struct Scan {
    pub items: Vec<ScannedItem>,
    pub scopes: BTreeMap<String, ModuleScope>,
}

impl Scan {
    fn extend(&mut self, other: Scan) {
        self.items.extend(other.items);
        self.scopes.extend(other.scopes);
    }
}

impl FromIterator<Scan> for Scan {
    fn from_iter<I: IntoIterator<Item = Scan>>(scans: I) -> Self {
        let mut all = Scan::default();
        for scan in scans {
            all.extend(scan);
        }
        all
    }
}

/// A struct/enum discovered during scanning, pending resolution against
/// manual trait impls found elsewhere in the crate.
struct PendingType {
    ident: String,
    file_path: String,
    module_path: String,
    /// Pipeline determined by a local `#[derive(...)]` or `#[apply(...)]`.
    /// `None` means the type only qualifies if a manual impl is found
    /// elsewhere in the crate.
    local_pipeline: Option<crate::types::Pipeline>,
    ast: ScannedAst,
}

/// A file skipped for carrying no Evenframe marker. It is parsed after all
/// only when it may define the target of a manual impl.
#[derive(Debug)]
struct DeferredFile {
    path: PathBuf,
    module_path: String,
}

/// State accumulated while scanning a crate (or a single file, in the test
/// path). A final [`Self::finalize`] merges pending types against manual
/// impls to produce the crate's [`ScannedItem`]s.
#[derive(Default)]
struct CrateScanState {
    pending: Vec<PendingType>,
    /// type ident → pipeline from a manual `impl EvenframeXxx for T` block.
    manual_impls: HashMap<String, crate::types::Pipeline>,
    deferred: Vec<DeferredFile>,
    scopes: BTreeMap<String, ModuleScope>,
}

impl CrateScanState {
    /// Every pending type that qualifies, with its configuration parsed, and
    /// the scopes of the modules scanned.
    fn finalize(self, resolve_only: bool) -> Scan {
        let CrateScanState {
            pending,
            manual_impls,
            scopes,
            ..
        } = self;
        let items = pending
            .into_iter()
            .filter_map(|pending_type| {
                let pipeline = pending_type
                    .local_pipeline
                    .or_else(|| manual_impls.get(&pending_type.ident).copied())?;
                let mut evenframe_type = EvenframeType {
                    name: pending_type.ident,
                    module_path: pending_type.module_path,
                    file_path: pending_type.file_path,
                    kind: match pending_type.ast {
                        ScannedAst::Struct(_) => TypeKind::Struct,
                        ScannedAst::Enum(_) => TypeKind::Enum,
                    },
                    has_id_field: false,
                    pipeline,
                    resolve_only,
                };
                let parsed = parse_scanned_item(
                    &pending_type.ast,
                    &evenframe_type,
                    &evenframe_type.file_path,
                );
                evenframe_type.has_id_field = matches!(
                    &parsed,
                    Ok(ParsedType::Struct { config, .. }) if has_database_id(config)
                );
                Some(ScannedItem {
                    evenframe_type,
                    parsed,
                })
            })
            .collect();
        Scan { items, scopes }
    }

    /// Records the scope of the module `items` make up at `module_path`, and
    /// of each inline module among them.
    fn record_scopes(&mut self, items: &[Item], module_path: &str) {
        self.scopes
            .insert(rust_path(module_path), ModuleScope::of(items, module_path));
        for item in items {
            if let Item::Mod(item_mod) = item
                && let Some((_, mod_items)) = &item_mod.content
            {
                self.record_scopes(mod_items, &format!("{module_path}::{}", item_mod.ident));
            }
        }
    }
}

/// Scanner for finding Evenframe types in a Rust workspace.
pub struct WorkspaceScanner {
    start_path: PathBuf,
    apply_aliases: Vec<String>,
    expand_macros: bool,
    /// Files outside the scan subtree to additionally parse after the directory
    /// walk. See [`Self::with_extra_files`].
    extra_files: Vec<IncludeFile>,
    /// Files in the scan subtree to leave out. See [`Self::with_excluded_files`].
    excluded_files: Vec<PathBuf>,
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
            extra_files: Vec::new(),
            excluded_files: Vec::new(),
        }
    }

    /// Leaves `files` (absolute paths) out of the directory walk, so their
    /// types are neither registered nor emitted by this scan.
    pub fn with_excluded_files(mut self, files: Vec<PathBuf>) -> Self {
        self.excluded_files = files.iter().map(|file| normalized(file)).collect();
        self
    }

    fn is_excluded(&self, path: &Path) -> bool {
        is_excluded(&self.excluded_files, path)
    }

    /// Adds files outside the scan subtree to parse after the directory walk.
    ///
    /// Each file is raw-parsed (never `cargo expand`ed) and its Evenframe types
    /// are appended to the scan results. Types from an entry with
    /// `resolve_only = true` are tagged so they are registered for field-type
    /// resolution but skipped at every emission site.
    pub fn with_extra_files(mut self, files: Vec<IncludeFile>) -> Self {
        self.extra_files = files;
        self
    }

    /// Scans for Rust workspaces and collects all Evenframe types within them.
    pub fn scan_for_evenframe_types(&self) -> Result<Vec<EvenframeType>> {
        Ok(self
            .scan()?
            .items
            .into_iter()
            .map(|item| item.evenframe_type)
            .collect())
    }

    /// Scans for Rust workspaces and returns every Evenframe type within
    /// them, each with its configuration parsed.
    ///
    /// Top-level crates are processed in parallel via rayon; each crate gets
    /// its own isolated scan state.
    pub fn scan(&self) -> Result<Scan> {
        info!(
            "Starting workspace scan for Evenframe types from path: {:?}",
            self.start_path
        );

        // First, collect all manifests we'll process (gitignored ones are
        // skipped; `include_files` below are read regardless).
        let manifests = find_manifests(&self.start_path);
        for manifest in &manifests {
            trace!("Found potential manifest: {:?}", manifest);
        }
        let known_manifests = canonical_manifests(&manifests);

        // Processing strategy depends on whether we're running `cargo expand`:
        //
        // - In `expand_macros` mode, each crate spawns `cargo expand --lib`
        //   which acquires the cargo build lock on `target/`. Running those
        //   in parallel makes them block on each other and can interleave
        //   builds in ways that corrupt the expansion output. Process
        //   sequentially. We also propagate errors instead of downgrading
        //   them to empty results: a corrupt expansion cache should be
        //   visible, not silently papered over.
        //
        // - In the raw-source path, there's no cargo contention and one
        //   broken manifest shouldn't kill the whole scan, so a crate that
        //   fails to scan is logged and skipped.
        let mut scanned: Scan = if self.expand_macros {
            let mut all = Scan::default();
            for manifest_path in &manifests {
                let items = self
                    .process_manifest(manifest_path, &known_manifests)
                    .map_err(|error| {
                        EvenframeError::WorkspaceScan(format!(
                            "expansion-mode scan failed at {:?}: {}",
                            manifest_path, error
                        ))
                    })?;
                all.extend(items);
            }
            all
        } else {
            manifests
                .par_iter()
                .map(
                    |manifest_path| match self.process_manifest(manifest_path, &known_manifests) {
                        Ok(items) => items,
                        Err(error) => {
                            warn!(
                                "Failed to process manifest at {:?}: {}",
                                manifest_path, error
                            );
                            Scan::default()
                        }
                    },
                )
                .collect::<Vec<Scan>>()
                .into_iter()
                .collect()
        };

        // Parse any `include_files` entries (files outside the scan subtree
        // whose types are registered so referencing fields resolve). Always
        // raw-parsed (never expanded); a bad path is surfaced as an error.
        for extra in &self.extra_files {
            let found = self.scan_extra_file(extra)?;
            debug!(
                "Included file {:?} contributed {} Evenframe types (resolve_only={})",
                extra.path,
                found.items.len(),
                extra.resolve_only
            );
            scanned.extend(found);
        }

        info!(
            "Workspace scan complete. Found {} Evenframe types",
            scanned.items.len()
        );
        Ok(scanned)
    }

    /// Parses a single included file (or a directory, scanned recursively like
    /// a crate `src` tree) outside the scan subtree, and returns its Evenframe
    /// types, each tagged with `extra.resolve_only`. The module path is
    /// derived from the file stem, so a path into the file from elsewhere
    /// resolves to its types by name.
    fn scan_extra_file(&self, extra: &IncludeFile) -> Result<Scan> {
        let abs = fs::canonicalize(&extra.path).map_err(|error| {
            EvenframeError::WorkspaceScan(format!(
                "include_files: cannot read {:?}: {}",
                extra.path, error
            ))
        })?;
        let module_path = abs
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("include")
            .to_string();

        let mut state = CrateScanState::default();
        if abs.is_dir() {
            // A directory entry is scanned recursively like a crate `src` tree,
            // so a whole sibling crate can join typesync with one entry instead
            // of enumerating every file.
            self.scan_directory_into(&abs, &mut state, &module_path, 0)?;
        } else {
            self.scan_rust_file_into(&abs, &mut state, &module_path)?;
        }
        self.finish_crate(state, extra.resolve_only)
    }

    /// Processes a Cargo.toml file, determines if it's a workspace or a single
    /// crate, and scans the corresponding source files. Returns the types
    /// found for this manifest only.
    fn process_manifest(
        &self,
        manifest_path: &Path,
        known_manifests: &HashSet<PathBuf>,
    ) -> Result<Scan> {
        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| EvenframeError::InvalidPath {
                path: manifest_path.to_path_buf(),
            })?;

        let content = fs::read_to_string(manifest_path)?;
        let manifest: toml::Value = toml::from_str(&content)
            .map_err(|error| EvenframeError::parse_error(manifest_path, error.to_string()))?;

        let mut out = Scan::default();

        // Check if this is a workspace manifest and scan its members.
        if let Some(workspace) = manifest.get("workspace").and_then(|w| w.as_table())
            && let Some(members) = workspace.get("members").and_then(|m| m.as_array())
        {
            debug!("Processing workspace at: {:?}", manifest_dir);

            for member in members.iter().filter_map(|member| member.as_str()) {
                let member_path = manifest_dir.join(member);
                if member_has_own_manifest(&member_path, known_manifests) {
                    debug!("Workspace member {member} is scanned as its own package");
                } else if member_path.is_dir() {
                    let crate_name = member_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("unknown_crate");
                    let src_path = member_path.join("src");
                    if src_path.exists() {
                        info!(
                            "Scanning workspace member: {} at {:?}",
                            crate_name, src_path
                        );
                        out.extend(self.scan_crate_sources(&src_path, crate_name)?);
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
                .and_then(|package| package.get("name"))
                .and_then(|name| name.as_str())
                .unwrap_or_else(|| {
                    manifest_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("unknown_crate")
                });

            if self.expand_macros {
                out.extend(self.scan_with_expansion_cache(manifest_dir, crate_name)?);
                return Ok(out);
            }

            let src_path = manifest_dir.join("src");
            if src_path.exists() {
                info!("Scanning crate: {} at {:?}", crate_name, src_path);
                out.extend(self.scan_crate_sources(&src_path, crate_name)?);
            }
        }

        Ok(out)
    }

    /// The raw-source scan of one crate's `src` tree.
    fn scan_crate_sources(&self, src_path: &Path, crate_name: &str) -> Result<Scan> {
        let mut state = CrateScanState::default();
        self.scan_directory_into(src_path, &mut state, crate_name, 0)?;
        self.finish_crate(state, false)
    }

    /// Parses the deferred files that may define a manual impl's target, then
    /// resolves the crate's pending types. A file with no Evenframe marker
    /// can only matter by defining such a target, so it is parsed when its
    /// text names one.
    fn finish_crate(&self, mut state: CrateScanState, resolve_only: bool) -> Result<Scan> {
        let deferred = std::mem::take(&mut state.deferred);
        if !state.manual_impls.is_empty() {
            for file in deferred {
                let content = fs::read_to_string(&file.path)?;
                if state
                    .manual_impls
                    .keys()
                    .any(|target| content.contains(target.as_str()))
                {
                    self.scan_source_into(&file.path, content, &mut state, &file.module_path)?;
                }
            }
        }
        Ok(state.finalize(resolve_only))
    }

    /// Expansion-based scan with per-file hash caching.
    ///
    /// Returns the extracted types on success. Any failure (`cargo expand`
    /// crashing, a source file that can't be hashed, a module missing from
    /// the split, a fragment write error) is propagated as an `Err` so the
    /// caller sees the corruption rather than silently falling back to the
    /// raw-source scan. The one silent-skip case is "crate has no `src/`
    /// directory", which we return as an empty scan.
    fn scan_with_expansion_cache(&self, manifest_dir: &Path, crate_name: &str) -> Result<Scan> {
        let src_path = manifest_dir.join("src");
        if !src_path.exists() {
            return Ok(Scan::default());
        }
        let src_dir = std::path::absolute(&src_path).map_err(|error| {
            EvenframeError::WorkspaceScan(format!(
                "failed to resolve {}: {error}",
                src_path.display()
            ))
        })?;

        // 1. Walk src/ and collect per-file metadata.
        let file_meta =
            collect_source_files(&src_path, crate_name, &self.excluded_files).map_err(|error| {
                EvenframeError::WorkspaceScan(format!(
                    "failed to walk src for '{}': {}",
                    crate_name, error
                ))
            })?;

        if file_meta.is_empty() {
            return Ok(Scan::default());
        }

        // 2. Hash files. Any hash failure is a hard error.
        let hashed: Vec<(SourceFile, String)> = file_meta
            .into_par_iter()
            .map(|meta| {
                let hash = expansion::hash_file(&meta.abs_path).map_err(|error| {
                    EvenframeError::WorkspaceScan(format!(
                        "hash failed for {:?}: {}",
                        meta.abs_path, error
                    ))
                })?;
                Ok((meta, hash))
            })
            .collect::<Result<Vec<_>>>()?;

        // 3. Load the existing manifest and bucket files by hit/miss. The
        //    recorded types hold only while `apply_aliases` is unchanged;
        //    otherwise a hit keeps its fragment and is re-extracted from it.
        let target_dir = expansion::find_target_dir(manifest_dir);
        let cache_dir = expansion::crate_cache_dir(&target_dir, crate_name);
        let manifest = CacheManifest::load(&cache_dir, crate_name);
        let types_current = manifest
            .as_ref()
            .is_some_and(|manifest| manifest.apply_aliases == self.apply_aliases);

        let file_modules: HashSet<String> = hashed
            .iter()
            .map(|(meta, _)| meta.module_path.clone())
            .collect();
        let mut hits: Vec<(SourceFile, String, CacheEntry)> = Vec::new();
        let mut misses: Vec<(SourceFile, String)> = Vec::new();
        for (meta, hash) in hashed {
            match manifest
                .as_ref()
                .and_then(|manifest| manifest.entries.get(&meta.rel_path))
            {
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

        // 4. Expand once for every miss. Any failure here is a hard error:
        //    falling back to a raw scan would silently mask corrupt state.
        let new_entries = if misses.is_empty() {
            Vec::new()
        } else {
            self.expand_misses(manifest_dir, crate_name, &cache_dir, &misses, &file_modules)?
        };

        // 5. Assemble the output: cache hits + freshly-expanded entries.
        let mut scanned = Scan::default();
        let mut next_manifest = CacheManifest::empty(crate_name, &src_dir, &self.apply_aliases);
        for (meta, hash, entry) in hits {
            let recorded = types_current.then(|| entry.recorded_scan()).flatten();
            let (items, entry) = match recorded {
                Some(items) => (items, entry),
                None => {
                    let items = match &entry.fragment_path {
                        Some(fragment_path) => {
                            let fragment = cache_dir.join(fragment_path);
                            let source = fs::read_to_string(&fragment)?;
                            self.extract_from_source(
                                source,
                                &entry.module_path,
                                &fragment.to_string_lossy(),
                            )?
                        }
                        None => Scan::default(),
                    };
                    let entry = CacheEntry::new(
                        entry.input_hash,
                        entry.module_path,
                        entry.fragment_path,
                        &items,
                    );
                    (items, entry)
                }
            };
            scanned.extend(items);
            next_manifest.entries.insert(
                meta.rel_path,
                CacheEntry {
                    input_hash: hash,
                    ..entry
                },
            );
        }
        for (rel_path, entry, items) in new_entries {
            scanned.extend(items);
            next_manifest.entries.insert(rel_path, entry);
        }

        next_manifest.save(&cache_dir).map_err(|error| {
            EvenframeError::WorkspaceScan(format!(
                "failed to save expansion manifest for '{}': {}",
                crate_name, error
            ))
        })?;

        Ok(scanned)
    }

    /// Expands the crate once and records an entry for every file in
    /// `misses`, scanning its expanded items directly. A miss whose module
    /// is absent from the expansion is a hard error: an empty entry cached
    /// in its place would drop the file's types on every later run.
    fn expand_misses(
        &self,
        manifest_dir: &Path,
        crate_name: &str,
        cache_dir: &Path,
        misses: &[(SourceFile, String)],
        file_modules: &HashSet<String>,
    ) -> Result<Vec<(String, CacheEntry, Scan)>> {
        let expanded = expansion::expand_crate(manifest_dir, crate_name)?;
        let parsed = parse_file(&expanded).map_err(|error| {
            EvenframeError::parse_error(Path::new("<expanded>"), error.to_string())
        })?;
        let mut by_file = expansion::split_by_file(parsed.items, crate_name, file_modules);
        misses
            .iter()
            .map(|(meta, hash)| {
                let items = by_file.remove(&meta.module_path).ok_or_else(|| {
                    EvenframeError::WorkspaceScan(format!(
                        "module '{}' (file {}) is not in the `cargo expand` output for crate \
                         '{}'; is the file declared with `mod`?",
                        meta.module_path, meta.rel_path, crate_name
                    ))
                })?;
                let fragment_path = if items.is_empty() {
                    None
                } else {
                    Some(expansion::write_fragment(
                        cache_dir,
                        &meta.rel_path,
                        &expansion::fragment_source(&items),
                    )?)
                };
                let file_path = fragment_path
                    .as_ref()
                    .map_or_else(
                        || meta.abs_path.clone(),
                        |fragment| cache_dir.join(fragment),
                    )
                    .to_string_lossy()
                    .to_string();
                let mut state = CrateScanState::default();
                self.scan_items_recursive(items, &mut state, &meta.module_path, &file_path);
                let scanned = state.finalize(false);
                let entry = CacheEntry::new(
                    hash.clone(),
                    meta.module_path.clone(),
                    fragment_path,
                    &scanned,
                );
                Ok((meta.rel_path.clone(), entry, scanned))
            })
            .collect()
    }

    /// Parses a cached fragment and extracts its Evenframe types, applying
    /// the same manual-impl merge as the raw-source path.
    fn extract_from_source(
        &self,
        source: String,
        module_path: &str,
        file_path: &str,
    ) -> Result<Scan> {
        let syntax_tree = parse_file(&source).map_err(|error| {
            EvenframeError::parse_error(Path::new(file_path), error.to_string())
        })?;
        let mut state = CrateScanState::default();
        self.scan_items_recursive(syntax_tree.items, &mut state, module_path, file_path);
        Ok(state.finalize(false))
    }

    /// Recursively scans a directory for Rust source files. Test-only
    /// wrapper that produces the finalized types for single-directory use.
    /// Production code uses [`Self::scan_directory_into`] directly so that
    /// manual impls in one file can resolve against structs in another.
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
        types.extend(
            self.finish_crate(state, false)?
                .items
                .into_iter()
                .map(|item| item.evenframe_type),
        );
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

        if depth > MAX_SCAN_DEPTH {
            return Err(EvenframeError::MaxRecursionDepth {
                depth: MAX_SCAN_DEPTH,
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
                let dir_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if dir_name != "tests" && dir_name != "benches" {
                    let module_path = format!("{}::{}", base_module, dir_name);
                    self.scan_directory_into(&path, state, &module_path, depth + 1)?;
                }
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
                if self.is_excluded(&path) {
                    debug!("Skipping excluded file: {:?}", path);
                    continue;
                }
                let file_stem = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("");

                if file_stem == "lib" || file_stem == "main" {
                    // Crate root, use the base module path directly.
                    self.scan_rust_file_into(&path, state, base_module)?;
                } else if path.file_name().and_then(|name| name.to_str()) == Some("mod.rs") {
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
    /// through this path; use the crate-level scanner for that.
    #[cfg(test)]
    fn scan_rust_file(
        &self,
        path: &Path,
        types: &mut Vec<EvenframeType>,
        module_path: &str,
    ) -> Result<()> {
        let mut state = CrateScanState::default();
        self.scan_rust_file_into(path, &mut state, module_path)?;
        types.extend(
            self.finish_crate(state, false)?
                .items
                .into_iter()
                .map(|item| item.evenframe_type),
        );
        Ok(())
    }

    /// Reads a single Rust file into the given [`CrateScanState`]. A file
    /// that carries no Evenframe marker is deferred unparsed: it can only
    /// contribute a type by defining the target of a manual impl, which
    /// [`Self::finish_crate`] checks once the crate is scanned.
    fn scan_rust_file_into(
        &self,
        path: &Path,
        state: &mut CrateScanState,
        module_path: &str,
    ) -> Result<()> {
        trace!("Scanning file: {:?}, module: {}", path, module_path);
        let content = fs::read_to_string(path)?;
        if !self.has_marker(&content) {
            // A file that only re-exports types still routes the paths that
            // name them, so its scope is kept.
            if reexports(&content) {
                let syntax_tree = parse_file(&content)
                    .map_err(|error| EvenframeError::parse_error(path, error.to_string()))?;
                state.record_scopes(&syntax_tree.items, module_path);
            }
            state.deferred.push(DeferredFile {
                path: path.to_path_buf(),
                module_path: module_path.to_string(),
            });
            return Ok(());
        }
        self.scan_source_into(path, content, state, module_path)
    }

    /// Parses `content`, read from `path`, and accumulates its structs, enums
    /// and manual trait impls into `state`.
    fn scan_source_into(
        &self,
        path: &Path,
        content: String,
        state: &mut CrateScanState,
        module_path: &str,
    ) -> Result<()> {
        let syntax_tree = parse_file(&content)
            .map_err(|error| EvenframeError::parse_error(path, error.to_string()))?;
        let file_path = path.to_string_lossy().to_string();
        self.scan_items_recursive(syntax_tree.items, state, module_path, &file_path);
        Ok(())
    }

    /// Whether `content` names anything that can make one of its types an
    /// Evenframe type: a derive, a manual impl, or an `apply` alias.
    fn has_marker(&self, content: &str) -> bool {
        ["Evenframe", "Typesync", "Schemasync"]
            .iter()
            .any(|marker| content.contains(marker))
            || self
                .apply_aliases
                .iter()
                .any(|alias| content.contains(alias.as_str()))
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
        items: Vec<Item>,
        state: &mut CrateScanState,
        module_path: &str,
        file_path: &str,
    ) {
        state
            .scopes
            .insert(rust_path(module_path), ModuleScope::of(&items, module_path));
        for item in items {
            match item {
                Item::Struct(item_struct) => {
                    let local_pipeline = detect_pipeline(&item_struct.attrs).or_else(|| {
                        self.has_apply_alias(&item_struct.attrs)
                            .then_some(crate::types::Pipeline::Both)
                    });
                    trace!(
                        "Collected struct candidate '{}' in '{}' (local_pipeline={:?})",
                        item_struct.ident, module_path, local_pipeline
                    );
                    state.pending.push(PendingType {
                        ident: item_struct.ident.to_string(),
                        file_path: file_path.to_string(),
                        module_path: module_path.to_string(),
                        local_pipeline,
                        ast: ScannedAst::Struct(item_struct),
                    });
                }
                Item::Enum(item_enum) => {
                    let local_pipeline = detect_pipeline(&item_enum.attrs).or_else(|| {
                        self.has_apply_alias(&item_enum.attrs)
                            .then_some(crate::types::Pipeline::Both)
                    });
                    trace!(
                        "Collected enum candidate '{}' in '{}' (local_pipeline={:?})",
                        item_enum.ident, module_path, local_pipeline
                    );
                    state.pending.push(PendingType {
                        ident: item_enum.ident.to_string(),
                        file_path: file_path.to_string(),
                        module_path: module_path.to_string(),
                        local_pipeline,
                        ast: ScannedAst::Enum(item_enum),
                    });
                }
                Item::Impl(item_impl) => {
                    if let Some((ident, pipeline)) = detect_manual_impl(&item_impl) {
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
                Item::Mod(item_mod) => {
                    if let Some((_, mod_items)) = item_mod.content {
                        let child_module = format!("{}::{}", module_path, item_mod.ident);
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
/// path (where its items sit in the crate's expansion) and a path relative
/// to the crate's `src/` directory (its cache manifest key).
#[derive(Debug)]
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
fn collect_source_files(
    src_path: &Path,
    crate_name: &str,
    excluded: &[PathBuf],
) -> Result<Vec<SourceFile>> {
    let mut out = Vec::new();
    walk_src(src_path, crate_name, "", excluded, &mut out, 0)?;
    Ok(out)
}

/// `path` without `.` components, for comparing configured paths with walked ones.
fn normalized(path: &Path) -> PathBuf {
    path.components().collect()
}

fn is_excluded(excluded: &[PathBuf], path: &Path) -> bool {
    !excluded.is_empty() && excluded.contains(&normalized(path))
}

fn walk_src(
    dir: &Path,
    base_module: &str,
    rel_dir: &str,
    excluded: &[PathBuf],
    out: &mut Vec<SourceFile>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_SCAN_DEPTH {
        return Err(EvenframeError::MaxRecursionDepth {
            depth: MAX_SCAN_DEPTH,
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
            // show up in `cargo expand --lib`. `bin/` is similar: each file
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
            walk_src(&path, &child_module, &child_rel, excluded, out, depth + 1)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let file_stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");
            // `src/main.rs` is the default binary target. It doesn't appear
            // in `cargo expand --lib` output, so including it would always
            // produce a missing-module error in the expansion path.
            if file_stem == "main" || is_excluded(excluded, &path) {
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
///   (`Validate` checks values and does not opt a type in on its own).
fn detect_manual_impl(item: &ItemImpl) -> Option<(String, crate::types::Pipeline)> {
    if item
        .attrs
        .iter()
        .any(|a| a.path().is_ident("automatically_derived"))
    {
        return None;
    }

    let (trait_path, _) = item.trait_.as_ref()?;
    let trait_name = trait_path.segments.last()?.ident.to_string();
    let pipeline = match trait_name.as_str() {
        "EvenframeTable"
        | "EvenframePersistableStruct"
        | "EvenframeAppStruct"
        | "EvenframeTaggedUnion" => crate::types::Pipeline::Both,
        _ => return None,
    };

    // Self type must be a bare ident (no path, no generics).
    let self_ty_ident = match item.self_ty.as_ref() {
        syn::Type::Path(tp) if tp.qself.is_none() && tp.path.segments.len() == 1 => {
            let seg = &tp.path.segments[0];
            if !matches!(seg.arguments, syn::PathArguments::None) {
                trace!(
                    "Skipping manual impl of '{}': self type has generic args",
                    trait_name
                );
                return None;
            }
            seg.ident.to_string()
        }
        _ => {
            trace!(
                "Skipping manual impl of '{}': self type is not a bare ident",
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

/// Whether `content` has a `pub use` declaration of any visibility, which can
/// make a module re-export a type other modules name it through.
fn reexports(content: &str) -> bool {
    content.lines().any(|line| {
        let line = line.trim_start();
        let after_visibility = if let Some(rest) = line.strip_prefix("pub(") {
            rest.split_once(')').map(|(_, rest)| rest)
        } else {
            line.strip_prefix("pub ")
        };
        after_visibility.is_some_and(|rest| rest.trim_start().starts_with("use "))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        EvenframeError, EvenframeType, HashMap, HashSet, IncludeFile, MAX_SCAN_DEPTH, Path,
        PathBuf, TypeKind, WorkspaceScanner, collect_source_files, detect_manual_impl,
        detect_pipeline,
    };
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
            resolve_only: false,
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
            resolve_only: false,
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
            resolve_only: false,
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
            resolve_only: false,
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
            resolve_only: false,
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
            assert_eq!(depth, MAX_SCAN_DEPTH);
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
            .process_manifest(&temp_dir.path().join("Cargo.toml"), &HashSet::new())
            .unwrap()
            .items;

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].evenframe_type.name, "User");
    }

    #[test]
    fn workspace_members_with_their_own_manifest_are_scanned_once() {
        let temp_dir = TempDir::new().unwrap();
        let member_src = temp_dir.path().join("member").join("src");
        fs::create_dir_all(&member_src).unwrap();
        create_rust_file(
            temp_dir.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        create_rust_file(
            &temp_dir.path().join("member"),
            "Cargo.toml",
            "[package]\nname = \"member_crate\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        create_rust_file(
            &member_src,
            "lib.rs",
            "#[derive(Evenframe)]\npub struct User {\n    pub id: String,\n}\n",
        )
        .unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let types = scanner.scan_for_evenframe_types().unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].module_path, "member_crate");
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
    fn test_detect_manual_impl_table_marker() {
        let item = parse_item_impl("impl EvenframeTable for User {}");
        assert_eq!(
            detect_manual_impl(&item),
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
    fn test_detect_manual_impl_ignores_validate() {
        let item = parse_item_impl(
            r#"
            impl Validate for Foo {
                fn validate(&self) -> Result<(), ValidationErrors> { Ok(()) }
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
        // No derive, only a manual impl.
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
        // Simulates derive macro output: no user-facing derive at all, only
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
    fn files_without_an_evenframe_marker_are_not_parsed() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        create_rust_file(
            &src_dir,
            "lib.rs",
            "#[derive(Evenframe)]\npub struct User { pub id: String }\n",
        )
        .unwrap();
        // Not valid Rust, and nothing in it can be an Evenframe type.
        create_rust_file(&src_dir, "helpers.rs", "pub fn broken( {\n").unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![], false);
        let mut types = Vec::new();
        scanner
            .scan_directory(&src_dir, &mut types, "test_crate", 0)
            .unwrap();
        assert_eq!(types.len(), 1, "{types:?}");
        assert_eq!(types[0].name, "User");
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
        // #[automatically_derived] in real expand output, but in raw source
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

        let files = collect_source_files(&src_dir, "my_crate", &[]).unwrap();
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

        let files = collect_source_files(&src_dir, "my_crate", &[]).unwrap();
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

        let files = collect_source_files(&src_dir, "my_crate", &[]).unwrap();
        let mut names: Vec<_> = files.iter().map(|f| f.rel_path.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["lib.rs".to_string(), "utils.rs".to_string()]);
    }

    #[test]
    fn excluded_files_are_left_out_of_the_scan() {
        let root = TempDir::new().unwrap();
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"exclusion\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        let src = root.path().join("src");
        fs::create_dir_all(&src).unwrap();
        create_rust_file(&src, "lib.rs", "mod kept;\nmod owned_elsewhere;\n").unwrap();
        create_rust_file(
            &src,
            "kept.rs",
            "#[derive(Evenframe)]\npub struct Kept { pub name: String }\n",
        )
        .unwrap();
        create_rust_file(
            &src,
            "owned_elsewhere.rs",
            "#[derive(Evenframe)]\npub struct OwnedElsewhere { pub name: String }\n",
        )
        .unwrap();

        let types = WorkspaceScanner::with_path(root.path().to_path_buf(), vec![], false)
            .with_excluded_files(vec![root.path().join("./src/owned_elsewhere.rs")])
            .scan_for_evenframe_types()
            .unwrap();

        let names: Vec<&str> = types.iter().map(|found| found.name.as_str()).collect();
        assert_eq!(names, ["Kept"]);
    }

    #[test]
    fn test_scan_extra_files_registers_external_types_with_resolve_only() {
        // Scan root is an empty dir with no manifest, so the only types come
        // from `include_files`. The external file lives OUTSIDE the scan root.
        let scan_root = TempDir::new().unwrap();
        let external_dir = TempDir::new().unwrap();

        let external_content = r#"
            #[derive(Debug, Clone, Evenframe)]
            pub struct AuthPolicy {
                pub password_mode: PasswordMode,
                pub max_sessions: i32,
            }

            #[derive(Debug, Clone, Evenframe)]
            pub enum PasswordMode {
                Disabled,
                Required,
            }
        "#;
        create_rust_file(external_dir.path(), "policy.rs", external_content).unwrap();
        let external_file = external_dir.path().join("policy.rs");

        // resolve_only = true: external types register, tagged resolve_only.
        let scanner = WorkspaceScanner::with_path(scan_root.path().to_path_buf(), vec![], false)
            .with_extra_files(vec![IncludeFile {
                path: external_file.clone(),
                resolve_only: true,
            }]);
        let types = scanner.scan_for_evenframe_types().unwrap();

        let auth_policy = types
            .iter()
            .find(|t| t.name == "AuthPolicy")
            .expect("AuthPolicy from include_files should be registered");
        assert_eq!(auth_policy.kind, TypeKind::Struct);
        assert!(auth_policy.resolve_only);
        // No `id` field → never materialized as a table.
        assert!(!auth_policy.has_id_field);

        let password_mode = types
            .iter()
            .find(|t| t.name == "PasswordMode")
            .expect("PasswordMode from include_files should be registered");
        assert_eq!(password_mode.kind, TypeKind::Enum);
        assert!(password_mode.resolve_only);

        // resolve_only = false: same types register, but NOT tagged resolve_only.
        let scanner = WorkspaceScanner::with_path(scan_root.path().to_path_buf(), vec![], false)
            .with_extra_files(vec![IncludeFile {
                path: external_file,
                resolve_only: false,
            }]);
        let types = scanner.scan_for_evenframe_types().unwrap();
        assert!(
            types
                .iter()
                .any(|t| t.name == "AuthPolicy" && !t.resolve_only)
        );
    }

    #[test]
    fn test_scan_extra_file_missing_path_errors() {
        // A typo'd include_files path must surface as an error, not be skipped.
        let scan_root = TempDir::new().unwrap();
        let scanner = WorkspaceScanner::with_path(scan_root.path().to_path_buf(), vec![], false)
            .with_extra_files(vec![IncludeFile {
                path: scan_root.path().join("does_not_exist.rs"),
                resolve_only: false,
            }]);
        assert!(scanner.scan_for_evenframe_types().is_err());
    }
}
