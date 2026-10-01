//! The scan cache: the resolved types from the last workspace scan,
//! persisted to `.evenframe/cache.json` so commands work without re-scanning
//! Rust sources that have not changed.
//!
//! The cache carries a fingerprint of everything the scan read (crate
//! manifests, Rust sources, include files, the evenframe config, plugins and,
//! when expanding macros, the lockfiles). Recomputing it takes a file walk and
//! one `stat` per input: a file whose length and timestamps match the last
//! stamp keeps its recorded hash, and only the rest are hashed. A cache whose
//! fingerprint no longer matches the sources is refused instead of used.
//!
//! It is a local cache: every scan that changes it rewrites it. Input paths
//! are project-relative, so moving the project keeps it valid, and content
//! hashes ignore CRLF vs LF.

use evenframe_core::error::{EvenframeError, Result};
use evenframe_core::scan::{
    AllConfigs, MAX_SCAN_DEPTH, ScanConfig, build_all_configs, canonical_manifests, find_manifests,
    member_has_own_manifest,
};
use evenframe_core::schemasync::TableConfig;
use evenframe_core::types::{StructConfig, TaggedUnion};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Bumped whenever the cache layout changes incompatibly.
pub const CACHE_FORMAT_VERSION: u32 = 4;

/// Where the cache lives, relative to the project root.
pub const CACHE_RELATIVE_PATH: &str = ".evenframe/cache.json";

/// The lightest command that refreshes the cache (a scan, with no database
/// connection), for staleness messages.
pub const CACHE_REFRESH_COMMAND: &str = "evenframe validate --types-only";

/// How close to the last stamping a file's timestamps may be and still have
/// its recorded hash reused. A write inside this window may share its
/// timestamp with the stamping on a coarse filesystem, so it is rehashed.
const RACY_WINDOW_NS: u128 = 2_000_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanCache {
    pub format_version: u32,
    pub evenframe_version: String,
    /// When the inputs were stamped, in nanoseconds since the Unix epoch.
    pub stamped_at_ns: u128,
    /// Every scan input, keyed by its path relative to the project root.
    pub inputs: BTreeMap<String, InputStamp>,
    pub enums: BTreeMap<String, TaggedUnion>,
    pub tables: BTreeMap<String, TableConfig>,
    pub objects: BTreeMap<String, StructConfig>,
}

/// One scan input as last seen: its content hash, and the metadata that lets
/// the next fingerprint reuse that hash without reading the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputStamp {
    pub len: u64,
    /// Absent where the platform keeps no modification time, which makes the
    /// hash unreusable.
    pub modified_ns: Option<u128>,
    /// The inode change time, which moves on any write even when the
    /// modification time is set back. Absent on platforms without one.
    pub changed_ns: Option<u128>,
    /// blake3 of the content with CRLF normalized to LF, or `"missing"` when
    /// the file cannot be read, so a vanished input changes the fingerprint.
    pub hash: String,
}

impl ScanCache {
    /// A cache entry holding `configs`, fingerprinted by `inputs` as stamped
    /// at `stamped_at_ns`.
    pub fn from_parts(
        stamped_at_ns: u128,
        inputs: BTreeMap<String, InputStamp>,
        configs: AllConfigs,
    ) -> Self {
        let (enums, tables, objects) = configs;
        Self {
            format_version: CACHE_FORMAT_VERSION,
            evenframe_version: env!("CARGO_PKG_VERSION").to_string(),
            stamped_at_ns,
            inputs,
            enums,
            tables,
            objects,
        }
    }

    /// The cache path for the project rooted at `project_root`.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(CACHE_RELATIVE_PATH)
    }

    /// Compact JSON. Maps are `BTreeMap`s all the way down, so the same scan
    /// always produces the same bytes.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|error| {
            EvenframeError::config(format!("Failed to serialize scan cache: {error}"))
        })
    }

    /// Write the cache under `project_root`, leaving the file untouched
    /// when its content wouldn't change. The write goes through a temporary
    /// sibling and a rename so readers never see a partial file.
    pub fn write(&self, project_root: &Path) -> Result<PathBuf> {
        let path = Self::path(project_root);
        let json = self.to_json()?;
        if fs::read_to_string(&path).is_ok_and(|existing| existing == json) {
            return Ok(path);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temp, &json)?;
        fs::rename(&temp, &path)?;
        Ok(path)
    }

    /// Load the cache under `project_root`, failing when there is none or
    /// it was written in another format.
    pub fn load(project_root: &Path) -> Result<Self> {
        match Self::read(project_root)? {
            ReadCache::Current(cache) => Ok(cache),
            ReadCache::Outdated(reason) => Err(EvenframeError::config(format!(
                "The scan cache at {} is out of date: {reason}; run `{CACHE_REFRESH_COMMAND}` to rebuild it",
                Self::path(project_root).display()
            ))),
        }
    }

    /// The cache file under `project_root`, or why its layout cannot be read
    /// by this evenframe.
    fn read(project_root: &Path) -> Result<ReadCache> {
        let path = Self::path(project_root);
        let json = fs::read_to_string(&path).map_err(|error| {
            EvenframeError::config(format!(
                "No scan cache at {} ({error}); run `{CACHE_REFRESH_COMMAND}` to create it",
                path.display()
            ))
        })?;
        match serde_json::from_str(&json) {
            Ok(cache) => Ok(ReadCache::Current(cache)),
            Err(error) => {
                match serde_json::from_str::<CacheVersion>(&json)
                    .ok()
                    .and_then(|written| written.change())
                {
                    Some(reason) => Ok(ReadCache::Outdated(reason)),
                    None => Err(EvenframeError::config(format!(
                        "Scan cache at {} is unreadable ({error}); run `{CACHE_REFRESH_COMMAND}` to rebuild it",
                        path.display()
                    ))),
                }
            }
        }
    }

    /// The cache under `project_root` when one exists and was written by this
    /// evenframe in this format, for a scan to reuse. Anything else means
    /// scanning afresh, and a cache that exists but cannot be used is logged.
    fn load_reusable(project_root: &Path) -> Option<Self> {
        if !Self::path(project_root).exists() {
            return None;
        }
        let reason = match Self::read(project_root) {
            Ok(ReadCache::Current(cache)) => match cache.version_change() {
                None => return Some(cache),
                Some(reason) => reason,
            },
            Ok(ReadCache::Outdated(reason)) => reason,
            Err(error) => error.to_string(),
        };
        tracing::info!("Rescanning: the scan cache cannot be reused: {reason}");
        None
    }

    /// Why a cache written by another evenframe or in another format cannot
    /// describe this project, or `None` when it could.
    fn version_change(&self) -> Option<String> {
        CacheVersion {
            format_version: self.format_version,
            evenframe_version: self.evenframe_version.clone(),
        }
        .change()
    }

    /// Why this cache no longer describes the project `config` points
    /// at, or `None` when it is current.
    pub fn stale_reason(&self, config: &ScanConfig) -> Result<Option<String>> {
        if let Some(reason) = self.version_change() {
            return Ok(Some(reason));
        }
        Ok(describe_changes(
            &self.inputs,
            &scan_inputs(config, Some(self))?,
        ))
    }

    /// Load the cache and fail with an actionable error if it is stale.
    pub fn load_current(config: &ScanConfig) -> Result<Self> {
        let cache = Self::load(&config.scan_path)?;
        if let Some(reason) = cache.stale_reason(config)? {
            return Err(EvenframeError::config(format!(
                "The scan cache is stale: {reason}. Run `{CACHE_REFRESH_COMMAND}` to refresh it."
            )));
        }
        Ok(cache)
    }

    /// Inspect the cache without failing on a missing or stale one, for
    /// callers that report the state rather than depend on it.
    pub fn status(config: &ScanConfig) -> Result<CacheStatus> {
        if !Self::path(&config.scan_path).exists() {
            return Ok(CacheStatus::Absent);
        }
        let cache = match Self::read(&config.scan_path)? {
            ReadCache::Current(cache) => cache,
            ReadCache::Outdated(reason) => return Ok(CacheStatus::Stale(reason)),
        };
        Ok(match cache.stale_reason(config)? {
            Some(reason) => CacheStatus::Stale(reason),
            None => CacheStatus::Current(cache),
        })
    }

    /// The scan results, in the shape `build_all_configs` returns.
    pub fn into_configs(self) -> AllConfigs {
        (self.enums, self.tables, self.objects)
    }
}

/// A cache file as read: usable, or in a layout this evenframe cannot load.
enum ReadCache {
    Current(ScanCache),
    Outdated(String),
}

/// The fields that say which layout a cache file has, readable whatever the
/// rest of it holds.
#[derive(Deserialize)]
struct CacheVersion {
    format_version: u32,
    evenframe_version: String,
}

impl CacheVersion {
    /// Why a cache written by another evenframe or in another format cannot
    /// be used here, or `None` when it can.
    fn change(&self) -> Option<String> {
        if self.format_version != CACHE_FORMAT_VERSION {
            return Some(format!(
                "the cache format changed (v{} → v{CACHE_FORMAT_VERSION})",
                self.format_version
            ));
        }
        let version = env!("CARGO_PKG_VERSION");
        (self.evenframe_version != version).then(|| {
            format!(
                "it was written by evenframe {} (this is {version})",
                self.evenframe_version
            )
        })
    }
}

/// How the cache on disk relates to the current sources, as
/// [`ScanCache::status`] finds it.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheStatus {
    /// The cache describes the sources as they are now.
    Current(ScanCache),
    /// No cache has been written for this project yet.
    Absent,
    /// The cache no longer describes the sources, for this reason.
    Stale(String),
}

/// The workspace's types: the cached ones when the cache still matches the
/// sources, otherwise a fresh scan, which is then recorded in the cache.
pub fn build_and_record(config: &ScanConfig) -> Result<AllConfigs> {
    let previous = ScanCache::load_reusable(&config.scan_path);
    let stamped_at_ns = now_ns();
    let inputs = scan_inputs(config, previous.as_ref())?;
    if let Some(cache) = previous {
        match describe_changes(&cache.inputs, &inputs) {
            None => {
                tracing::debug!("Scan cache is current; skipping the scan");
                return Ok(cache.into_configs());
            }
            Some(changes) => tracing::debug!("Rescanning: {changes}"),
        }
    }
    let cache = ScanCache::from_parts(stamped_at_ns, inputs, build_all_configs(config)?);
    let path = cache.write(&config.scan_path)?;
    tracing::debug!("Scan cache written to {}", path.display());
    Ok(cache.into_configs())
}

/// Stamp every file a scan with `config` reads, reusing the hashes in
/// `previous` for files whose metadata has not moved. Mirrors the discovery
/// rules of [`WorkspaceScanner`](evenframe_core::scan::WorkspaceScanner) without parsing any Rust: the
/// non-gitignored `Cargo.toml`s under the scan path (see
/// [`find_manifests`]), the `src` trees of their packages and
/// workspace members (minus `tests`/`benches` directories and symlinks), the
/// include files (gitignored or not), the config file, any plugin binaries
/// and, when expanding macros, the lockfile each crate builds against.
pub fn scan_inputs(
    config: &ScanConfig,
    previous: Option<&ScanCache>,
) -> Result<BTreeMap<String, InputStamp>> {
    let root = &config.scan_path;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut lockfiles: BTreeSet<PathBuf> = BTreeSet::new();

    let manifests = find_manifests(root);
    let known_manifests = canonical_manifests(&manifests);
    for manifest in &manifests {
        files.push(manifest.clone());
        let manifest_dir = manifest
            .parent()
            .ok_or_else(|| EvenframeError::InvalidPath {
                path: manifest.clone(),
            })?;
        let text = fs::read_to_string(manifest).map_err(|error| {
            EvenframeError::WorkspaceScan(format!("failed to read {}: {error}", manifest.display()))
        })?;
        let value: toml::Value = toml::from_str(&text)
            .map_err(|error| EvenframeError::parse_error(manifest, error.to_string()))?;
        if let Some(members) = value
            .get("workspace")
            .and_then(|workspace| workspace.get("members"))
            .and_then(|members| members.as_array())
        {
            for member in members.iter().filter_map(|member| member.as_str()) {
                let member_dir = manifest_dir.join(member);
                if !member_has_own_manifest(&member_dir, &known_manifests) {
                    collect_src_files(&member_dir.join("src"), &mut files)?;
                }
            }
        }
        if value.get("package").is_some() {
            collect_src_files(&manifest_dir.join("src"), &mut files)?;
            if config.expand_macros
                && let Some(lockfile) = governing_lockfile(manifest_dir)
            {
                lockfiles.insert(lockfile);
            }
        }
    }
    files.extend(lockfiles);

    for include in &config.include_files {
        if include.path.is_dir() {
            collect_rust_files(&include.path, &mut files, 0)?;
        } else {
            files.push(include.path.clone());
        }
    }

    if let Some(config_path) = &config.config_path {
        files.push(config_path.clone());
    }
    let plugin_paths = config
        .output_rule_plugins
        .values()
        .map(|plugin| &plugin.path)
        .chain(
            config
                .synthetic_item_plugins
                .values()
                .map(|plugin| &plugin.path),
        );
    for plugin in plugin_paths {
        files.push(root.join(plugin));
    }

    let reusable = previous.map(|cache| (cache.stamped_at_ns, &cache.inputs));
    Ok(files
        .into_par_iter()
        .map(|file| {
            let key = relative_key(root, &file);
            let recorded = reusable.and_then(|(stamped_at_ns, inputs)| {
                inputs.get(&key).map(|stamp| (stamped_at_ns, stamp))
            });
            let stamp = stamp_input(&file, recorded);
            (key, stamp)
        })
        .collect())
}

/// The `Cargo.lock` the crate at `manifest_dir` resolves its dependencies
/// with: the nearest one at or above it.
fn governing_lockfile(manifest_dir: &Path) -> Option<PathBuf> {
    manifest_dir
        .ancestors()
        .map(|dir| dir.join("Cargo.lock"))
        .find(|lockfile| lockfile.is_file())
}

/// A crate's `src` tree; like the scanner, a crate without one contributes
/// nothing.
fn collect_src_files(src: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if src.is_dir() {
        collect_rust_files(src, out, 0)?;
    }
    Ok(())
}

/// The `.rs` files under `dir`, failing where the scanner would: on an
/// unreadable directory or past [`MAX_SCAN_DEPTH`].
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > MAX_SCAN_DEPTH {
        return Err(EvenframeError::MaxRecursionDepth {
            depth: MAX_SCAN_DEPTH,
            path: dir.to_path_buf(),
        });
    }
    let unreadable = |error: std::io::Error| {
        EvenframeError::WorkspaceScan(format!("failed to read {}: {error}", dir.display()))
    };
    let mut paths = fs::read_dir(dir)
        .map_err(unreadable)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<PathBuf>>>()
        .map_err(unreadable)?;
    paths.sort();
    for path in paths {
        let metadata = path.symlink_metadata().map_err(|error| {
            EvenframeError::WorkspaceScan(format!("failed to read {}: {error}", path.display()))
        })?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name != "tests" && name != "benches" {
                collect_rust_files(&path, out, depth + 1)?;
            }
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// A `/`-separated path relative to `root` (or the path itself when it lies
/// outside the root).
fn relative_key(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn now_ns() -> u128 {
    // A clock before the epoch only makes every recorded hash unreusable.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

fn since_epoch_ns(time: std::io::Result<SystemTime>) -> Option<u128> {
    time.ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_nanos())
}

#[cfg(unix)]
fn changed_ns(metadata: &fs::Metadata) -> Option<u128> {
    use std::os::unix::fs::MetadataExt;
    let seconds = u128::try_from(metadata.ctime()).ok()?;
    let nanos = u128::try_from(metadata.ctime_nsec()).ok()?;
    Some(seconds * 1_000_000_000 + nanos)
}

#[cfg(not(unix))]
fn changed_ns(_metadata: &fs::Metadata) -> Option<u128> {
    None
}

/// The stamp of the file at `path`. `recorded` is its stamp from the last
/// fingerprint and when that was taken; its hash is reused when the length
/// and both timestamps are unchanged and lie clearly before that moment.
fn stamp_input(path: &Path, recorded: Option<(u128, &InputStamp)>) -> InputStamp {
    let Ok(metadata) = fs::metadata(path) else {
        return missing_stamp();
    };
    let len = metadata.len();
    let modified_ns = since_epoch_ns(metadata.modified());
    let changed_ns = changed_ns(&metadata);
    if let Some((stamped_at_ns, stamp)) = recorded {
        let settled = |time_ns: u128| time_ns + RACY_WINDOW_NS < stamped_at_ns;
        if stamp.len == len
            && stamp.modified_ns == modified_ns
            && stamp.changed_ns == changed_ns
            && modified_ns.is_some_and(settled)
            && changed_ns.is_none_or(settled)
        {
            return stamp.clone();
        }
    }
    match fs::read(path) {
        Ok(bytes) => InputStamp {
            len,
            modified_ns,
            changed_ns,
            hash: content_hash(&bytes),
        },
        Err(_) => missing_stamp(),
    }
}

fn missing_stamp() -> InputStamp {
    InputStamp {
        len: 0,
        modified_ns: None,
        changed_ns: None,
        hash: "missing".to_string(),
    }
}

/// blake3 of `bytes` with CRLF line endings normalized to LF. Only content
/// that holds a `\r` is copied to normalize it.
fn content_hash(bytes: &[u8]) -> String {
    if !bytes.contains(&b'\r') {
        return blake3::hash(bytes).to_hex().to_string();
    }
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some((&byte, tail)) = rest.split_first() {
        if !(byte == b'\r' && tail.first() == Some(&b'\n')) {
            normalized.push(byte);
        }
        rest = tail;
    }
    blake3::hash(&normalized).to_hex().to_string()
}

/// A short description of how `current` differs from `recorded`, naming the
/// file when only one changed.
fn describe_changes(
    recorded: &BTreeMap<String, InputStamp>,
    current: &BTreeMap<String, InputStamp>,
) -> Option<String> {
    let changed: Vec<&String> = current
        .iter()
        .filter(|(path, stamp)| {
            recorded
                .get(*path)
                .is_some_and(|old| old.hash != stamp.hash)
        })
        .map(|(path, _)| path)
        .collect();
    let added: Vec<&String> = current
        .keys()
        .filter(|path| !recorded.contains_key(*path))
        .collect();
    let removed: Vec<&String> = recorded
        .keys()
        .filter(|path| !current.contains_key(*path))
        .collect();

    let mut parts = Vec::new();
    for (paths, singular, plural) in [
        (&changed, "changed", "changed"),
        (&added, "was added", "added"),
        (&removed, "was removed", "removed"),
    ] {
        match paths.as_slice() {
            [] => {}
            [only] => parts.push(format!("{only} {singular}")),
            many => parts.push(format!("{} files {plural}", many.len())),
        }
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::{
        BTreeMap, CACHE_FORMAT_VERSION, CACHE_REFRESH_COMMAND, CacheStatus, EvenframeError,
        InputStamp, MAX_SCAN_DEPTH, RACY_WINDOW_NS, ScanCache, ScanConfig, build_all_configs,
        build_and_record, fs, now_ns, scan_inputs,
    };
    use tempfile::TempDir;

    /// A cache of a fresh scan of `config`'s project.
    fn scanned(config: &ScanConfig) -> ScanCache {
        ScanCache::from_parts(
            now_ns(),
            scan_inputs(config, None).unwrap(),
            build_all_configs(config).unwrap(),
        )
    }

    fn hashes(inputs: &BTreeMap<String, InputStamp>) -> BTreeMap<&str, &str> {
        inputs
            .iter()
            .map(|(path, stamp)| (path.as_str(), stamp.hash.as_str()))
            .collect()
    }

    fn project() -> (TempDir, ScanConfig) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("src/models")).unwrap();
        fs::create_dir_all(root.join("src/tests")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "#[derive(Evenframe)]\npub struct User { pub id: String }\n",
        )
        .unwrap();
        fs::write(root.join("src/models/post.rs"), "pub struct Post;\n").unwrap();
        fs::write(root.join("src/tests/ignored.rs"), "").unwrap();
        fs::write(root.join("evenframe.toml"), "[general]\n").unwrap();
        let config = ScanConfig {
            scan_path: root.to_path_buf(),
            config_path: Some(root.join("evenframe.toml")),
            ..ScanConfig::default()
        };
        (tmp, config)
    }

    #[test]
    fn inputs_cover_manifests_sources_and_config() {
        let (_tmp, config) = project();
        let inputs = scan_inputs(&config, None).unwrap();
        let keys: Vec<&str> = inputs.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec![
                "Cargo.toml",
                "evenframe.toml",
                "src/lib.rs",
                "src/models/post.rs"
            ]
        );
    }

    #[test]
    fn workspace_members_are_fingerprinted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/api\", \"crates/db\"]\n",
        )
        .unwrap();
        for (member, module) in [("api", "routes"), ("db", "models")] {
            let dir = root.join("crates").join(member);
            fs::create_dir_all(dir.join("src").join(module)).unwrap();
            fs::create_dir_all(dir.join("src/tests")).unwrap();
            fs::write(
                dir.join("Cargo.toml"),
                format!("[package]\nname = \"{member}\"\nversion = \"0.0.0\"\n"),
            )
            .unwrap();
            fs::write(dir.join("src/lib.rs"), "").unwrap();
            fs::write(dir.join("src").join(module).join("user.rs"), "").unwrap();
            fs::write(dir.join("src/tests/skipped.rs"), "").unwrap();
        }
        let config = ScanConfig {
            scan_path: root.to_path_buf(),
            ..ScanConfig::default()
        };
        let inputs = scan_inputs(&config, None).unwrap();
        insta::assert_debug_snapshot!(inputs.keys().collect::<Vec<_>>());
    }

    #[test]
    fn unparseable_manifest_is_an_error() {
        let (tmp, config) = project();
        fs::write(tmp.path().join("Cargo.toml"), "[package\n").unwrap();
        let err = scan_inputs(&config, None).unwrap_err();
        assert!(matches!(err, EvenframeError::ParseError { .. }), "{err}");
    }

    #[test]
    fn sources_past_the_scan_depth_are_an_error() {
        let (tmp, config) = project();
        let mut dir = tmp.path().join("src");
        for level in 0..=MAX_SCAN_DEPTH {
            dir = dir.join(format!("level_{level}"));
        }
        fs::create_dir_all(&dir).unwrap();
        let err = scan_inputs(&config, None).unwrap_err();
        assert!(
            matches!(err, EvenframeError::MaxRecursionDepth { .. }),
            "{err}"
        );
    }

    #[test]
    fn gitignored_crates_are_skipped_unless_included() {
        let (tmp, mut config) = project();
        let root = tmp.path();
        fs::write(root.join(".gitignore"), "/generated/\n").unwrap();
        fs::create_dir_all(root.join("generated/src")).unwrap();
        fs::write(
            root.join("generated/Cargo.toml"),
            "[package]\nname = \"generated\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            root.join("generated/src/lib.rs"),
            "#[derive(Evenframe)]\npub struct Hidden { pub id: String }\n",
        )
        .unwrap();

        let inputs = scan_inputs(&config, None).unwrap();
        assert!(
            !inputs.keys().any(|k| k.starts_with("generated/")),
            "gitignored crate fingerprinted: {inputs:?}"
        );
        let (_, tables, _) = build_all_configs(&config).unwrap();
        assert!(!tables.contains_key("hidden"), "gitignored crate scanned");

        config.include_files = vec![evenframe_core::config::IncludeFile {
            path: root.join("generated/src/lib.rs"),
            resolve_only: false,
        }];
        let inputs = scan_inputs(&config, None).unwrap();
        assert!(inputs.contains_key("generated/src/lib.rs"), "{inputs:?}");
        let (_, tables, _) = build_all_configs(&config).unwrap();
        assert!(tables.contains_key("hidden"), "included file not scanned");
    }

    #[test]
    fn line_endings_do_not_change_the_hash() {
        let (tmp, config) = project();
        let before = scan_inputs(&config, None).unwrap();
        fs::write(
            tmp.path().join("src/lib.rs"),
            "#[derive(Evenframe)]\r\npub struct User { pub id: String }\r\n",
        )
        .unwrap();
        assert_eq!(
            hashes(&scan_inputs(&config, None).unwrap()),
            hashes(&before)
        );
    }

    #[test]
    fn cache_round_trips_and_detects_staleness() {
        let (tmp, config) = project();
        let cache = scanned(&config);
        assert!(cache.tables.contains_key("user"));

        let path = cache.write(tmp.path()).unwrap();
        assert_eq!(path, tmp.path().join(".evenframe/cache.json"));
        let json = fs::read_to_string(&path).unwrap();
        assert!(!json.contains('\n'), "the cache should be compact");
        assert!(
            !json.contains(&*tmp.path().to_string_lossy()),
            "absolute paths leaked"
        );

        let loaded = ScanCache::load_current(&config).unwrap();
        assert_eq!(loaded, cache);

        fs::write(tmp.path().join("src/models/post.rs"), "pub struct Post2;\n").unwrap();
        assert_eq!(
            loaded.stale_reason(&config).unwrap().as_deref(),
            Some("src/models/post.rs changed")
        );

        fs::write(tmp.path().join("src/models/comment.rs"), "").unwrap();
        fs::remove_file(tmp.path().join("src/lib.rs")).unwrap();
        assert_eq!(
            loaded.stale_reason(&config).unwrap().as_deref(),
            Some(
                "src/models/post.rs changed, src/models/comment.rs was added, src/lib.rs was removed"
            )
        );

        let err = ScanCache::load_current(&config).unwrap_err().to_string();
        assert!(
            err.contains(&format!("Run `{CACHE_REFRESH_COMMAND}`")),
            "{err}"
        );
    }

    #[test]
    fn status_reports_absent_current_and_stale() {
        let (tmp, config) = project();
        assert_eq!(ScanCache::status(&config).unwrap(), CacheStatus::Absent);

        let cache = scanned(&config);
        cache.write(tmp.path()).unwrap();
        assert_eq!(
            ScanCache::status(&config).unwrap(),
            CacheStatus::Current(cache)
        );

        fs::write(tmp.path().join("src/models/post.rs"), "pub struct Post2;\n").unwrap();
        assert_eq!(
            ScanCache::status(&config).unwrap(),
            CacheStatus::Stale("src/models/post.rs changed".to_string())
        );
    }

    #[test]
    fn version_mismatch_is_stale() {
        let (_tmp, config) = project();
        let mut cache = ScanCache::from_parts(
            now_ns(),
            scan_inputs(&config, None).unwrap(),
            (BTreeMap::new(), BTreeMap::new(), BTreeMap::new()),
        );
        cache.evenframe_version = "0.0.1".to_string();
        let reason = cache.stale_reason(&config).unwrap().unwrap();
        assert!(reason.contains("written by evenframe 0.0.1"), "{reason}");
    }

    #[test]
    fn missing_cache_explains_how_to_create_it() {
        let (tmp, _config) = project();
        let err = ScanCache::load(tmp.path()).unwrap_err().to_string();
        assert!(
            err.contains(&format!("run `{CACHE_REFRESH_COMMAND}` to create it")),
            "{err}"
        );
    }

    #[test]
    fn an_unchanged_project_is_served_from_the_cache() {
        let (tmp, config) = project();
        let (_, tables, _) = build_and_record(&config).unwrap();
        assert!(tables.contains_key("user"));

        // Edit the recorded types without touching any input: a run that
        // returns the edit read the cache instead of scanning.
        let mut cache = ScanCache::load(tmp.path()).unwrap();
        cache.tables.clear();
        cache.write(tmp.path()).unwrap();
        let (_, tables, _) = build_and_record(&config).unwrap();
        assert!(tables.is_empty(), "the scan ran although nothing changed");

        fs::write(
            tmp.path().join("src/models/post.rs"),
            "#[derive(Evenframe)]\npub struct Post { pub id: String }\n",
        )
        .unwrap();
        let (_, tables, _) = build_and_record(&config).unwrap();
        assert!(tables.contains_key("user") && tables.contains_key("post"));
    }

    #[test]
    fn a_settled_file_keeps_its_recorded_hash() {
        let (tmp, config) = project();
        let mut previous = scanned(&config);
        previous.stamped_at_ns = now_ns() + 10 * RACY_WINDOW_NS;
        for stamp in previous.inputs.values_mut() {
            stamp.hash = "recorded".to_string();
        }

        let inputs = scan_inputs(&config, Some(&previous)).unwrap();
        assert!(inputs.values().all(|stamp| stamp.hash == "recorded"));

        fs::write(tmp.path().join("src/models/post.rs"), "pub struct Pst;\n").unwrap();
        let inputs = scan_inputs(&config, Some(&previous)).unwrap();
        assert_ne!(inputs["src/models/post.rs"].hash, "recorded");
        assert_eq!(inputs["src/lib.rs"].hash, "recorded");
    }

    #[test]
    fn a_file_stamped_inside_the_racy_window_is_rehashed() {
        let (_tmp, config) = project();
        let mut previous = scanned(&config);
        previous.stamped_at_ns = now_ns();
        for stamp in previous.inputs.values_mut() {
            stamp.hash = "recorded".to_string();
        }
        let inputs = scan_inputs(&config, Some(&previous)).unwrap();
        assert!(inputs.values().all(|stamp| stamp.hash != "recorded"));
    }

    #[test]
    fn a_cache_in_an_older_format_is_stale_and_rebuilt() {
        let (tmp, config) = project();
        let path = ScanCache::path(tmp.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let older = format!(
            "{{\"format_version\":1,\"evenframe_version\":\"{}\",\"inputs\":{{\"src/lib.rs\":\"hash\"}},\"enums\":{{}},\"tables\":{{}},\"objects\":{{}}}}",
            env!("CARGO_PKG_VERSION")
        );
        fs::write(&path, older).unwrap();
        match ScanCache::status(&config).unwrap() {
            CacheStatus::Stale(reason) => assert!(reason.contains("format changed"), "{reason}"),
            other => panic!("expected a stale cache, got {other:?}"),
        }
        let (_, tables, _) = build_and_record(&config).unwrap();
        assert!(tables.contains_key("user"));
        assert_eq!(
            ScanCache::load(tmp.path()).unwrap().format_version,
            CACHE_FORMAT_VERSION
        );
    }

    #[test]
    fn expanding_macros_fingerprints_the_lockfile() {
        let (tmp, mut config) = project();
        fs::write(tmp.path().join("Cargo.lock"), "version = 4\n").unwrap();
        assert!(
            !scan_inputs(&config, None)
                .unwrap()
                .contains_key("Cargo.lock")
        );
        config.expand_macros = true;
        assert!(
            scan_inputs(&config, None)
                .unwrap()
                .contains_key("Cargo.lock")
        );
    }
}
