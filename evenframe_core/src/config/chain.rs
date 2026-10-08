//! A configuration is every config file from the outermost ancestor down to
//! the nearest one, merged with the nearer file winning. A bare config in a
//! project directory thereby defers everything it leaves out to the configs
//! above it.
//!
//! Each file is resolved on its own before the merge: its `.env` is loaded,
//! its environment references substituted, and its relative paths joined to
//! its own project root, so a path means the same thing whichever config ends
//! up supplying it.

use super::{EvenframeConfig, load_env_from};
use crate::error::{EvenframeError, Result};
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// One config file of a chain, as written.
pub(super) struct Link {
    pub path: PathBuf,
    pub contents: String,
    pub document: Table,
}

impl Link {
    pub fn read(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path).map_err(|error| {
            EvenframeError::config(format!(
                "Failed to read configuration file {}: {error}",
                path.display()
            ))
        })?;
        Self::parse(path.to_path_buf(), contents)
    }

    pub fn parse(path: PathBuf, contents: String) -> Result<Self> {
        let document = contents.parse::<Table>().map_err(|error| {
            EvenframeError::config(format!(
                "Failed to parse configuration file {}: {error}",
                path.display()
            ))
        })?;
        Ok(Self {
            path,
            contents,
            document,
        })
    }

    fn project_root(&self) -> &Path {
        EvenframeConfig::project_root_of(&self.path)
    }

    fn config_dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// The `.env` file this config names, as [`EvenframeConfig::resolve_env_path`]
    /// resolves it.
    fn env_path(&self) -> PathBuf {
        let written = self
            .document
            .get("general")
            .and_then(|general| general.get("env_path"))
            .and_then(Value::as_str);
        env_path_for(&self.path, written)
    }

    /// The projects this config declares as a workspace root, if any.
    pub fn projects(&self) -> Option<Vec<String>> {
        let projects = self.document.get("general")?.get("projects")?.as_array()?;
        Some(
            projects
                .iter()
                .filter_map(|project| project.as_str().map(str::to_string))
                .collect(),
        )
    }
}

/// The `.env` file for the config at `config_path`: `env_path` relative to the
/// config file's directory, or the project root's `.env` when unset.
pub(super) fn env_path_for(config_path: &Path, env_path: Option<&str>) -> PathBuf {
    let raw = match env_path {
        Some(custom) => config_path.parent().unwrap_or(Path::new(".")).join(custom),
        None => EvenframeConfig::project_root_of(config_path).join(".env"),
    };
    std::path::absolute(&raw).unwrap_or(raw)
}

/// The config file directly in `dir`: `.evenframe/config.toml` (preferred) or
/// `evenframe.toml`.
pub(super) fn config_in(dir: &Path) -> Option<PathBuf> {
    let dotdir_config = dir.join(".evenframe").join("config.toml");
    if dotdir_config.is_file() {
        return Some(dotdir_config);
    }
    let legacy_config = dir.join("evenframe.toml");
    legacy_config.is_file().then_some(legacy_config)
}

/// Every config file in `start` and its ancestors, outermost first.
pub fn find_config_chain_from(start: &Path) -> Vec<PathBuf> {
    let mut chain: Vec<PathBuf> = start.ancestors().filter_map(config_in).collect();
    chain.reverse();
    chain
}

/// The chain that ends at `nearest`: the config files of its project root's
/// ancestors, outermost first, then `nearest` itself.
pub fn chain_ending_at(nearest: &Path) -> Vec<PathBuf> {
    let mut chain = EvenframeConfig::project_root_of(nearest)
        .parent()
        .map(find_config_chain_from)
        .unwrap_or_default();
    chain.push(nearest.to_path_buf());
    chain
}

/// Loads every link's `.env`, nearest first, so a nearer file's value wins
/// over an outer one's: a variable already set is never overridden.
pub(super) fn load_envs(links: &[Link]) {
    for link in links.iter().rev() {
        load_env_from(&link.env_path());
    }
}

/// Merges `links` (outermost first) into one configuration whose file is the
/// last link's.
pub(super) fn merge_links(
    links: Vec<Link>,
    require_connection_env: bool,
) -> Result<EvenframeConfig> {
    let Some(nearest) = links.last() else {
        return Err(EvenframeError::config("No configuration file to load"));
    };
    let config_path = nearest.path.clone();
    // A lone file is read as written first, so a malformed setting is
    // reported with its position in the file.
    if let [only] = links.as_slice() {
        toml::from_str::<EvenframeConfig>(&only.contents).map_err(|error| {
            EvenframeError::config(format!(
                "Failed to parse configuration file {}: {error}",
                only.path.display()
            ))
        })?;
    }
    load_envs(&links);

    let files: Vec<String> = links
        .iter()
        .map(|link| link.path.display().to_string())
        .collect();
    let last = links.len() - 1;
    let mut merged = Table::new();
    for (index, link) in links.into_iter().enumerate() {
        let project_root = link.project_root().to_path_buf();
        let config_dir = link.config_dir().to_path_buf();
        let mut document = Value::Table(link.document);
        EvenframeConfig::substitute_strings(&mut document, &[], !require_connection_env)?;
        let Value::Table(mut document) = document else {
            return Err(EvenframeError::config(format!(
                "{} is not a TOML table",
                link.path.display()
            )));
        };
        absolutize_paths(&mut document, &project_root, &config_dir)?;
        // A workspace's projects belong to the config that declares them.
        if index != last
            && let Some(Value::Table(general)) = document.get_mut("general")
        {
            general.remove("projects");
        }
        merge(&mut merged, document);
    }

    let mut config: EvenframeConfig = Value::Table(merged).try_into().map_err(|error| {
        EvenframeError::config(format!(
            "Failed to read configuration from {}: {error}",
            files.join(", ")
        ))
    })?;
    config.config_file_path = config_path;
    Ok(config)
}

/// `general.foreign_types` of `links` (outermost first), merged nearer-wins,
/// with no `.env` loaded and nothing substituted.
pub(super) fn merged_foreign_types(
    links: Vec<Link>,
) -> Result<std::collections::BTreeMap<String, super::ForeignTypeConfig>> {
    #[derive(serde::Deserialize, Default)]
    struct General {
        #[serde(default, deserialize_with = "super::deserialize_foreign_types")]
        foreign_types: std::collections::BTreeMap<String, super::ForeignTypeConfig>,
    }
    let files: Vec<String> = links
        .iter()
        .map(|link| link.path.display().to_string())
        .collect();
    let mut merged = Table::new();
    for link in links {
        if let Some(Value::Table(foreign_types)) = link
            .document
            .get("general")
            .and_then(|general| general.get("foreign_types"))
        {
            let mut nearer = Table::new();
            nearer.insert(
                "foreign_types".to_string(),
                Value::Table(foreign_types.clone()),
            );
            merge(&mut merged, nearer);
        }
    }
    let general: General = Value::Table(merged).try_into().map_err(|error| {
        EvenframeError::config(format!(
            "Failed to read foreign_types from {}: {error}",
            files.join(", ")
        ))
    })?;
    Ok(general.foreign_types)
}

/// Merges `nearer` into `base`: tables merge key by key, and any other value
/// replaces what `base` had.
fn merge(base: &mut Table, nearer: Table) {
    for (key, value) in nearer {
        // `output` and `outputs` are two spellings of one setting, so either
        // replaces both.
        if let ("typesync", Value::Table(typesync)) = (key.as_str(), &value)
            && (typesync.contains_key("output") || typesync.contains_key("outputs"))
            && let Some(Value::Table(base_typesync)) = base.get_mut("typesync")
        {
            base_typesync.remove("output");
            base_typesync.remove("outputs");
        }
        match (base.get_mut(&key), value) {
            (Some(Value::Table(existing)), Value::Table(table)) => merge(existing, table),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Joins every relative path setting in `document` to the directory it is
/// relative to: `general.env_path` to `config_dir`, everything else to
/// `project_root`.
fn absolutize_paths(document: &mut Table, project_root: &Path, config_dir: &Path) -> Result<()> {
    if let Some(Value::Table(general)) = document.get_mut("general") {
        if let Some(env_path) = general.get_mut("env_path") {
            absolutize(env_path, config_dir)?;
        }
        if let Some(Value::Array(include_files)) = general.get_mut("include_files") {
            for entry in include_files {
                match entry {
                    Value::Table(spec) => absolutize_key(spec, "path", project_root)?,
                    other => absolutize(other, project_root)?,
                }
            }
        }
        if let Some(Value::Array(exclude_files)) = general.get_mut("exclude_files") {
            for entry in exclude_files {
                absolutize(entry, project_root)?;
            }
        }
        for plugins in ["output_rule_plugins", "synthetic_item_plugins"] {
            absolutize_each_path(general.get_mut(plugins), project_root)?;
        }
    }
    if let Some(Value::Table(schemasync)) = document.get_mut("schemasync") {
        absolutize_each_path(schemasync.get_mut("plugins"), project_root)?;
        if let Some(Value::Table(database)) = schemasync.get_mut("database") {
            for source in ["accesses", "functions", "analyzers"] {
                if let Some(Value::Table(source)) = database.get_mut(source) {
                    absolutize_key(source, "path", project_root)?;
                }
            }
        }
    }
    if let Some(Value::Table(typesync)) = document.get_mut("typesync") {
        if let Some(Value::Table(output)) = typesync.get_mut("output") {
            absolutize_key(output, "dir", project_root)?;
        }
        if let Some(Value::Array(outputs)) = typesync.get_mut("outputs") {
            for output in outputs {
                if let Value::Table(output) = output {
                    absolutize_key(output, "dir", project_root)?;
                }
            }
        }
    }
    Ok(())
}

/// The `path` of every entry in a table of named entries, such as plugins.
fn absolutize_each_path(entries: Option<&mut Value>, base: &Path) -> Result<()> {
    if let Some(Value::Table(entries)) = entries {
        for (_, entry) in entries.iter_mut() {
            if let Value::Table(entry) = entry {
                absolutize_key(entry, "path", base)?;
            }
        }
    }
    Ok(())
}

fn absolutize_key(table: &mut Table, key: &str, base: &Path) -> Result<()> {
    match table.get_mut(key) {
        Some(value) => absolutize(value, base),
        None => Ok(()),
    }
}

/// `value` joined to `base` when it is a relative path. A value of another
/// type is left for deserialization to report.
fn absolutize(value: &mut Value, base: &Path) -> Result<()> {
    let Value::String(text) = value else {
        return Ok(());
    };
    if Path::new(text.as_str()).is_absolute() {
        return Ok(());
    }
    // Without `.` components, so `./generated` reads as `<root>/generated`.
    let joined: PathBuf = base.join(text.as_str()).components().collect();
    let Some(joined) = joined.to_str() else {
        return Err(EvenframeError::config(format!(
            "{} is not valid UTF-8",
            joined.display()
        )));
    };
    *text = joined.to_string();
    Ok(())
}

#[cfg(test)]
#[path = "chain_tests.rs"]
mod tests;
