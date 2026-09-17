//! The scan registry: the resolved types from the last workspace scan,
//! persisted to `.evenframe/registry.json` so commands such as
//! `evenframe mockmake` can work without re-scanning Rust sources.
//!
//! The registry carries a fingerprint of everything the scan read (crate
//! manifests, Rust sources, include files, the evenframe config and
//! plugins). [`ScanRegistry::stale_reason`] recomputes it with a cheap file
//! walk — no Rust parsing — so a registry that no longer matches the sources
//! is refused instead of silently used.
//!
//! The file is meant to be committed: it is pretty-printed JSON with sorted
//! keys and a trailing newline, holds project-relative `/` paths, and has no
//! timestamps. Content hashes ignore CRLF vs LF.

use super::{AllConfigs, BuildConfig};
use crate::error::{EvenframeError, Result};
use crate::schemasync::TableConfig;
use crate::types::{StructConfig, TaggedUnion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Bumped whenever the registry layout changes incompatibly.
pub const REGISTRY_FORMAT_VERSION: u32 = 1;

/// Where the registry lives, relative to the project root.
pub const REGISTRY_RELATIVE_PATH: &str = ".evenframe/registry.json";

/// The command that refreshes the registry, for staleness messages.
const REFRESH_COMMAND: &str = "evenframe schemasync";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanRegistry {
    pub format_version: u32,
    pub evenframe_version: String,
    /// Content hash (blake3) of every scan input, keyed by its path relative
    /// to the project root.
    pub inputs: BTreeMap<String, String>,
    pub enums: BTreeMap<String, TaggedUnion>,
    pub tables: BTreeMap<String, TableConfig>,
    pub objects: BTreeMap<String, StructConfig>,
}

impl ScanRegistry {
    /// Build a registry for the scan results `configs`, fingerprinting the
    /// inputs `config` describes.
    pub fn from_scan(config: &BuildConfig, configs: &AllConfigs) -> Result<Self> {
        let (enums, tables, objects) = configs;
        Ok(Self {
            format_version: REGISTRY_FORMAT_VERSION,
            evenframe_version: env!("CARGO_PKG_VERSION").to_string(),
            inputs: scan_inputs(config)?,
            enums: enums.clone(),
            tables: tables.clone(),
            objects: objects.clone(),
        })
    }

    /// The registry path for the project rooted at `project_root`.
    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(REGISTRY_RELATIVE_PATH)
    }

    /// Serialize as committed: pretty JSON with a trailing newline. Maps are
    /// `BTreeMap`s all the way down, so key order is stable.
    pub fn to_json(&self) -> Result<String> {
        let mut json = serde_json::to_string_pretty(self)
            .map_err(|e| EvenframeError::config(format!("Failed to serialize registry: {e}")))?;
        json.push('\n');
        Ok(json)
    }

    /// Write the registry under `project_root`, leaving the file untouched
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

    /// Load the registry under `project_root`.
    pub fn load(project_root: &Path) -> Result<Self> {
        let path = Self::path(project_root);
        let json = fs::read_to_string(&path).map_err(|e| {
            EvenframeError::config(format!(
                "No scan registry at {} ({e}); run `{REFRESH_COMMAND}` to create it",
                path.display()
            ))
        })?;
        serde_json::from_str(&json).map_err(|e| {
            EvenframeError::config(format!(
                "Scan registry at {} is unreadable ({e}); run `{REFRESH_COMMAND}` to rebuild it",
                path.display()
            ))
        })
    }

    /// Why this registry no longer describes the project `config` points
    /// at, or `None` when it is current.
    pub fn stale_reason(&self, config: &BuildConfig) -> Result<Option<String>> {
        if self.format_version != REGISTRY_FORMAT_VERSION {
            return Ok(Some(format!(
                "the registry format changed (v{} → v{REGISTRY_FORMAT_VERSION})",
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

    /// Load the registry and fail with an actionable error if it is stale.
    pub fn load_current(config: &BuildConfig) -> Result<Self> {
        let registry = Self::load(&config.scan_path)?;
        if let Some(reason) = registry.stale_reason(config)? {
            return Err(EvenframeError::config(format!(
                "The scan registry is stale: {reason}. Run `{REFRESH_COMMAND}` to refresh it."
            )));
        }
        Ok(registry)
    }

    /// The scan results, in the shape `build_all_configs` returns.
    pub fn into_configs(self) -> AllConfigs {
        (self.enums, self.tables, self.objects)
    }
}

/// Run the workspace scan and record its results in the registry.
pub fn build_and_record(config: &BuildConfig) -> Result<AllConfigs> {
    let configs = super::build_all_configs(config)?;
    let registry = ScanRegistry::from_scan(config, &configs)?;
    let path = registry.write(&config.scan_path)?;
    tracing::debug!("Scan registry written to {}", path.display());
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
        let Some(manifest_dir) = manifest.parent() else {
            continue;
        };
        let Ok(value) = fs::read_to_string(manifest)
            .map_err(|_| ())
            .and_then(|text| toml::from_str::<toml::Value>(&text).map_err(|_| ()))
        else {
            continue;
        };
        if let Some(members) = value
            .get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
        {
            for member in members.iter().filter_map(|m| m.as_str()) {
                collect_rust_files(&manifest_dir.join(member).join("src"), &mut files, 0);
            }
        }
        if value.get("package").is_some() {
            collect_rust_files(&manifest_dir.join("src"), &mut files, 0);
        }
    }

    for include in &config.include_files {
        if include.path.is_dir() {
            collect_rust_files(&include.path, &mut files, 0);
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

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 10 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name != "tests" && name != "benches" {
                collect_rust_files(&path, out, depth + 1);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
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
    fn registry_round_trips_and_detects_staleness() {
        let (tmp, config) = project();
        let configs = super::super::build_all_configs(&config).unwrap();
        let registry = ScanRegistry::from_scan(&config, &configs).unwrap();
        assert!(registry.tables.contains_key("user"));

        let path = registry.write(tmp.path()).unwrap();
        assert_eq!(path, tmp.path().join(".evenframe/registry.json"));
        let json = fs::read_to_string(&path).unwrap();
        assert!(json.ends_with("}\n"));
        assert!(
            !json.contains(&*tmp.path().to_string_lossy()),
            "absolute paths leaked"
        );

        let loaded = ScanRegistry::load_current(&config).unwrap();
        assert_eq!(loaded, registry);

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

        let err = ScanRegistry::load_current(&config).unwrap_err().to_string();
        assert!(err.contains("Run `evenframe schemasync`"), "{err}");
    }

    #[test]
    fn version_mismatch_is_stale() {
        let (_tmp, config) = project();
        let mut registry = ScanRegistry::from_scan(
            &config,
            &(BTreeMap::new(), BTreeMap::new(), BTreeMap::new()),
        )
        .unwrap();
        registry.evenframe_version = "0.0.1".to_string();
        let reason = registry.stale_reason(&config).unwrap().unwrap();
        assert!(reason.contains("written by evenframe 0.0.1"), "{reason}");
    }

    #[test]
    fn missing_registry_explains_how_to_create_it() {
        let (tmp, _config) = project();
        let err = ScanRegistry::load(tmp.path()).unwrap_err().to_string();
        assert!(
            err.contains("run `evenframe schemasync` to create it"),
            "{err}"
        );
    }
}
