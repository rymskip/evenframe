//! The scan cache: the resolved types from the last workspace scan,
//! persisted to `.evenframe/cache.json` so commands such as
//! `evenframe mockmake` can work without re-scanning Rust sources.
//!
//! The cache carries a fingerprint of everything the scan read (crate
//! manifests, Rust sources, include files, the evenframe config and
//! plugins). [`ScanCache::stale_reason`] recomputes it with a cheap file
//! walk — no Rust parsing — so a cache that no longer matches the sources
//! is refused instead of silently used.
//!
//! It is a local cache, never committed: every scan rewrites it. Input paths
//! are project-relative, so moving the project keeps it valid, and content
//! hashes ignore CRLF vs LF.

use super::{AllConfigs, BuildConfig, MAX_SCAN_DEPTH};
use crate::error::{EvenframeError, Result};
use crate::schemasync::TableConfig;
use crate::types::{StructConfig, TaggedUnion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Bumped whenever the cache layout changes incompatibly.
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// Where the cache lives, relative to the project root.
pub const CACHE_RELATIVE_PATH: &str = ".evenframe/cache.json";

/// The lightest command that refreshes the cache (a scan, with no database
/// connection), for staleness messages.
pub const CACHE_REFRESH_COMMAND: &str = "evenframe validate --types-only";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanCache {
    pub format_version: u32,
    pub evenframe_version: String,
    /// Content hash (blake3) of every scan input, keyed by its path relative
    /// to the project root.
    pub inputs: BTreeMap<String, String>,
    pub enums: BTreeMap<String, TaggedUnion>,
    pub tables: BTreeMap<String, TableConfig>,
    pub objects: BTreeMap<String, StructConfig>,
}

impl ScanCache {
    /// Build a cache entry for the scan results `configs`, fingerprinting the
    /// inputs `config` describes.
    pub fn from_scan(config: &BuildConfig, configs: &AllConfigs) -> Result<Self> {
        let (enums, tables, objects) = configs;
        Ok(Self {
            format_version: CACHE_FORMAT_VERSION,
            evenframe_version: env!("CARGO_PKG_VERSION").to_string(),
            inputs: scan_inputs(config)?,
            enums: enums.clone(),
            tables: tables.clone(),
            objects: objects.clone(),
        })
    }

    /// The cache path for the project rooted at `project_root`.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(CACHE_RELATIVE_PATH)
    }

    /// Compact JSON. Maps are `BTreeMap`s all the way down, so the same scan
    /// always produces the same bytes and an unchanged cache is not rewritten.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self)
            .map_err(|e| EvenframeError::config(format!("Failed to serialize scan cache: {e}")))
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

    /// Load the cache under `project_root`.
    pub fn load(project_root: &Path) -> Result<Self> {
        let path = Self::path(project_root);
        let json = fs::read_to_string(&path).map_err(|e| {
            EvenframeError::config(format!(
                "No scan cache at {} ({e}); run `{CACHE_REFRESH_COMMAND}` to create it",
                path.display()
            ))
        })?;
        serde_json::from_str(&json).map_err(|e| {
            EvenframeError::config(format!(
                "Scan cache at {} is unreadable ({e}); run `{CACHE_REFRESH_COMMAND}` to rebuild it",
                path.display()
            ))
        })
    }

    /// Why this cache no longer describes the project `config` points
    /// at, or `None` when it is current.
    pub fn stale_reason(&self, config: &BuildConfig) -> Result<Option<String>> {
        if self.format_version != CACHE_FORMAT_VERSION {
            return Ok(Some(format!(
                "the cache format changed (v{} → v{CACHE_FORMAT_VERSION})",
                self.format_version
            )));
        }
        let version = env!("CARGO_PKG_VERSION");
        if self.evenframe_version != version {
            return Ok(Some(format!(
                "it was written by evenframe {} (this is {version})",
                self.evenframe_version
            )));
        }
        Ok(describe_changes(&self.inputs, &scan_inputs(config)?))
    }

    /// Load the cache and fail with an actionable error if it is stale.
    pub fn load_current(config: &BuildConfig) -> Result<Self> {
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
    pub fn status(config: &BuildConfig) -> Result<CacheStatus> {
        if !Self::path(&config.scan_path).exists() {
            return Ok(CacheStatus::Absent);
        }
        let cache = Self::load(&config.scan_path)?;
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

/// Run the workspace scan and record its results in the cache.
pub fn build_and_record(config: &BuildConfig) -> Result<AllConfigs> {
    let configs = super::build_all_configs(config)?;
    let cache = ScanCache::from_scan(config, &configs)?;
    let path = cache.write(&config.scan_path)?;
    tracing::debug!("Scan cache written to {}", path.display());
    Ok(configs)
}

/// Hash every file a scan with `config` reads. Mirrors the discovery rules
/// of [`super::WorkspaceScanner`] without parsing any Rust: the
/// non-gitignored `Cargo.toml`s under the scan path (see
/// [`super::find_manifests`]), the `src` trees of their packages and
/// workspace members (minus `tests`/`benches` directories and symlinks), the
/// include files (gitignored or not), the config file and any plugin
/// binaries.
pub fn scan_inputs(config: &BuildConfig) -> Result<BTreeMap<String, String>> {
    let root = &config.scan_path;
    let mut files: Vec<PathBuf> = Vec::new();

    let manifests = super::find_manifests(root);
    for manifest in &manifests {
        files.push(manifest.clone());
        let manifest_dir = manifest
            .parent()
            .ok_or_else(|| EvenframeError::InvalidPath {
                path: manifest.clone(),
            })?;
        let text = fs::read_to_string(manifest).map_err(|e| {
            EvenframeError::WorkspaceScan(format!("failed to read {}: {e}", manifest.display()))
        })?;
        let value: toml::Value = toml::from_str(&text)
            .map_err(|e| EvenframeError::parse_error(manifest, e.to_string()))?;
        if let Some(members) = value
            .get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
        {
            for member in members.iter().filter_map(|m| m.as_str()) {
                collect_src_files(&manifest_dir.join(member).join("src"), &mut files)?;
            }
        }
        if value.get("package").is_some() {
            collect_src_files(&manifest_dir.join("src"), &mut files)?;
        }
    }

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
        .map(|p| &p.path)
        .chain(config.synthetic_item_plugins.values().map(|p| &p.path));
    for plugin in plugin_paths {
        files.push(root.join(plugin));
    }

    let mut inputs = BTreeMap::new();
    for file in files {
        let key = relative_key(root, &file);
        inputs.insert(key, hash_input(&file));
    }
    Ok(inputs)
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
    let unreadable = |e: std::io::Error| {
        EvenframeError::WorkspaceScan(format!("failed to read {}: {e}", dir.display()))
    };
    let mut paths = fs::read_dir(dir)
        .map_err(unreadable)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<PathBuf>>>()
        .map_err(unreadable)?;
    paths.sort();
    for path in paths {
        let metadata = path.symlink_metadata().map_err(|e| {
            EvenframeError::WorkspaceScan(format!("failed to read {}: {e}", path.display()))
        })?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name != "tests" && name != "benches" {
                collect_rust_files(&path, out, depth + 1)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
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
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// blake3 of the file's content with CRLF line endings normalized, or
/// `"missing"` when it can't be read (so a vanished input changes the hash).
fn hash_input(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => blake3::hash(text.replace("\r\n", "\n").as_bytes())
                .to_hex()
                .to_string(),
            Err(e) => blake3::hash(e.as_bytes()).to_hex().to_string(),
        },
        Err(_) => "missing".to_string(),
    }
}

/// A short description of how `current` differs from `recorded`, naming the
/// file when only one changed.
fn describe_changes(
    recorded: &BTreeMap<String, String>,
    current: &BTreeMap<String, String>,
) -> Option<String> {
    let changed: Vec<&String> = current
        .iter()
        .filter(|(path, hash)| recorded.get(*path).is_some_and(|old| old != *hash))
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
    use super::*;
    use tempfile::TempDir;

    fn project() -> (TempDir, BuildConfig) {
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
        let config = BuildConfig {
            scan_path: root.to_path_buf(),
            config_path: Some(root.join("evenframe.toml")),
            ..BuildConfig::default()
        };
        (tmp, config)
    }

    #[test]
    fn inputs_cover_manifests_sources_and_config() {
        let (_tmp, config) = project();
        let inputs = scan_inputs(&config).unwrap();
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
        let config = BuildConfig {
            scan_path: root.to_path_buf(),
            ..BuildConfig::default()
        };
        let inputs = scan_inputs(&config).unwrap();
        insta::assert_debug_snapshot!(inputs.keys().collect::<Vec<_>>());
    }

    #[test]
    fn unparseable_manifest_is_an_error() {
        let (tmp, config) = project();
        fs::write(tmp.path().join("Cargo.toml"), "[package\n").unwrap();
        let err = scan_inputs(&config).unwrap_err();
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
        let err = scan_inputs(&config).unwrap_err();
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

        let inputs = scan_inputs(&config).unwrap();
        assert!(
            !inputs.keys().any(|k| k.starts_with("generated/")),
            "gitignored crate fingerprinted: {inputs:?}"
        );
        let (_, tables, _) = super::super::build_all_configs(&config).unwrap();
        assert!(!tables.contains_key("hidden"), "gitignored crate scanned");

        config.include_files = vec![super::super::IncludeFile {
            path: root.join("generated/src/lib.rs"),
            resolve_only: false,
        }];
        let inputs = scan_inputs(&config).unwrap();
        assert!(inputs.contains_key("generated/src/lib.rs"), "{inputs:?}");
        let (_, tables, _) = super::super::build_all_configs(&config).unwrap();
        assert!(tables.contains_key("hidden"), "included file not scanned");
    }

    #[test]
    fn line_endings_do_not_change_the_hash() {
        let (tmp, config) = project();
        let before = scan_inputs(&config).unwrap();
        fs::write(
            tmp.path().join("src/lib.rs"),
            "#[derive(Evenframe)]\r\npub struct User { pub id: String }\r\n",
        )
        .unwrap();
        assert_eq!(scan_inputs(&config).unwrap(), before);
    }

    #[test]
    fn cache_round_trips_and_detects_staleness() {
        let (tmp, config) = project();
        let configs = super::super::build_all_configs(&config).unwrap();
        let cache = ScanCache::from_scan(&config, &configs).unwrap();
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

        let configs = super::super::build_all_configs(&config).unwrap();
        let cache = ScanCache::from_scan(&config, &configs).unwrap();
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
        let mut cache = ScanCache::from_scan(
            &config,
            &(BTreeMap::new(), BTreeMap::new(), BTreeMap::new()),
        )
        .unwrap();
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
}
