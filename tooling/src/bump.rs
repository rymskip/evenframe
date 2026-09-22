//! `tooling bump`: raise the shared version of every published workspace crate,
//! repin the dependencies on them, and refresh every tracked lockfile.

use crate::{header, project_root, run};
use semver::Version;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::{DocumentMut, Item, TableLike, Value};

const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum BumpLevel {
    Major,
    Minor,
    Patch,
}

struct Manifest {
    path: PathBuf,
    document: DocumentMut,
}

impl Manifest {
    fn read(path: PathBuf) -> Result<Self, String> {
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("reading {}: {error}", path.display()))?;
        let document = text
            .parse::<DocumentMut>()
            .map_err(|error| format!("parsing {}: {error}", path.display()))?;
        Ok(Self { path, document })
    }

    fn write(&self) -> Result<(), String> {
        fs::write(&self.path, self.document.to_string())
            .map_err(|error| format!("writing {}: {error}", self.path.display()))
    }

    fn package_str(&self, key: &str) -> Option<&str> {
        self.document.get("package")?.get(key)?.as_str()
    }

    fn is_published(&self) -> bool {
        self.document
            .get("package")
            .and_then(|package| package.get("publish"))
            .and_then(Item::as_bool)
            != Some(false)
    }
}

pub fn cmd_bump(level: BumpLevel) -> bool {
    match bump(level) {
        Ok(version) => {
            println!("\n=== bumped to {version} ===");
            true
        }
        Err(message) => {
            eprintln!("bump failed: {message}");
            false
        }
    }
}

fn bump(level: BumpLevel) -> Result<Version, String> {
    let root = project_root();
    let mut manifests = tracked_files(&root, "Cargo.toml")?
        .into_iter()
        .map(Manifest::read)
        .collect::<Result<Vec<_>, _>>()?;

    let members = workspace_members(&root, &manifests)?;
    let published: Vec<usize> = manifests
        .iter()
        .enumerate()
        .filter(|(_, manifest)| members.contains(&manifest.path) && manifest.is_published())
        .map(|(index, _)| index)
        .collect();
    let current = shared_version(published.iter().map(|&index| &manifests[index]))?;
    let next = next_version(&current, level)?;
    let next_text = next.to_string();
    let published_names = published
        .iter()
        .map(|&index| {
            manifests[index]
                .package_str("name")
                .map(str::to_string)
                .ok_or_else(|| format!("{} has no package.name", manifests[index].path.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;

    header(&format!("bumping {current} -> {next}"));
    for (index, manifest) in manifests.iter_mut().enumerate() {
        let mut changed = pin_dependencies(manifest, &published_names, &next_text)?;
        if published.contains(&index) {
            let version = manifest
                .document
                .get_mut("package")
                .and_then(Item::as_table_like_mut)
                .and_then(|package| package.get_mut("version"))
                .ok_or_else(|| format!("{} has no package.version", manifest.path.display()))?;
            if !set_string(version, &next_text) {
                return Err(format!(
                    "{}: package.version is not a string",
                    manifest.path.display()
                ));
            }
            changed = true;
        }
        if changed {
            manifest.write()?;
            println!("updated {}", relative(&root, &manifest.path).display());
        }
    }

    for lockfile in tracked_files(&root, "Cargo.lock")? {
        let directory = lockfile
            .parent()
            .ok_or_else(|| format!("{} has no parent directory", lockfile.display()))?;
        header(&format!(
            "refreshing {}",
            relative(&root, &lockfile).display()
        ));
        if !run("cargo", |command| {
            command
                .args(["update", "--workspace", "--offline"])
                .current_dir(directory);
        }) {
            return Err(format!("refreshing {} failed", lockfile.display()));
        }
    }
    Ok(next)
}

/// Every file tracked by git whose name is exactly `file_name`, as absolute paths.
fn tracked_files(root: &Path, file_name: &str) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(|error| format!("running git ls-files: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-files failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let mut files = Vec::new();
    for entry in output
        .stdout
        .split(|&byte| byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let path = std::str::from_utf8(entry)
            .map_err(|error| format!("git ls-files returned a non-UTF-8 path: {error}"))?;
        let path = root.join(path);
        if path.file_name().is_some_and(|name| name == file_name) {
            files.push(path);
        }
    }
    Ok(files)
}

fn workspace_members(root: &Path, manifests: &[Manifest]) -> Result<Vec<PathBuf>, String> {
    let root_path = root.join("Cargo.toml");
    let root_manifest = manifests
        .iter()
        .find(|manifest| manifest.path == root_path)
        .ok_or_else(|| format!("{} is not tracked", root_path.display()))?;
    let members = root_manifest
        .document
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(Item::as_array)
        .ok_or_else(|| format!("{} has no workspace.members array", root_path.display()))?;
    members
        .iter()
        .map(|member| {
            let member = member
                .as_str()
                .ok_or_else(|| "workspace.members holds a non-string entry".to_string())?;
            let path = root.join(member).join("Cargo.toml");
            if manifests.iter().any(|manifest| manifest.path == path) {
                Ok(path)
            } else {
                Err(format!(
                    "workspace member `{member}` has no tracked Cargo.toml (glob members are not supported)"
                ))
            }
        })
        .collect()
}

fn shared_version<'a>(published: impl Iterator<Item = &'a Manifest>) -> Result<Version, String> {
    let mut shared: Option<(Version, &Path)> = None;
    for manifest in published {
        let text = manifest.package_str("version").ok_or_else(|| {
            format!(
                "{} has no literal package.version (workspace-inherited versions are not supported)",
                manifest.path.display()
            )
        })?;
        let version = Version::parse(text).map_err(|error| {
            format!(
                "{}: package.version {text}: {error}",
                manifest.path.display()
            )
        })?;
        match &shared {
            None => shared = Some((version, manifest.path.as_path())),
            Some((existing, existing_path)) if *existing != version => {
                return Err(format!(
                    "published crates disagree on version: {} is {existing}, {} is {version}",
                    existing_path.display(),
                    manifest.path.display()
                ));
            }
            Some(_) => {}
        }
    }
    shared
        .map(|(version, _)| version)
        .ok_or_else(|| "the workspace has no published crates".to_string())
}

fn next_version(current: &Version, level: BumpLevel) -> Result<Version, String> {
    if !current.pre.is_empty() || !current.build.is_empty() {
        return Err(format!(
            "{current} carries pre-release or build metadata, which bump does not support"
        ));
    }
    Ok(match level {
        BumpLevel::Major => Version::new(current.major + 1, 0, 0),
        BumpLevel::Minor => Version::new(current.major, current.minor + 1, 0),
        BumpLevel::Patch => Version::new(current.major, current.minor, current.patch + 1),
    })
}

/// Repin every versioned dependency on a published crate, in every dependency
/// table of the manifest. Returns whether anything was repinned.
fn pin_dependencies(manifest: &mut Manifest, names: &[String], next: &str) -> Result<bool, String> {
    let mut changed = false;
    for table in dependency_tables(&mut manifest.document) {
        for (key, dependency) in table.iter_mut() {
            let crate_name = dependency
                .get("package")
                .and_then(Item::as_str)
                .unwrap_or(key.get());
            if !names.iter().any(|name| name == crate_name) {
                continue;
            }
            let version = if dependency.is_str() {
                Some(dependency)
            } else {
                dependency
                    .as_table_like_mut()
                    .and_then(|fields| fields.get_mut("version"))
            };
            let Some(version) = version else {
                continue;
            };
            if !set_string(version, next) {
                return Err(format!(
                    "{}: dependency `{}` has a non-string version",
                    manifest.path.display(),
                    key.get()
                ));
            }
            changed = true;
        }
    }
    Ok(changed)
}

/// The top-level, `workspace` and per-`target` dependency tables of a manifest.
fn dependency_tables(document: &mut DocumentMut) -> Vec<&mut dyn TableLike> {
    let mut tables = Vec::new();
    for (key, item) in document.as_table_mut().iter_mut() {
        match key.get() {
            name if DEPENDENCY_TABLES.contains(&name) => tables.extend(item.as_table_like_mut()),
            "workspace" => tables.extend(
                item.as_table_like_mut()
                    .and_then(|workspace| workspace.get_mut("dependencies"))
                    .and_then(Item::as_table_like_mut),
            ),
            "target" => {
                let Some(targets) = item.as_table_like_mut() else {
                    continue;
                };
                for (_, target) in targets.iter_mut() {
                    let Some(target) = target.as_table_like_mut() else {
                        continue;
                    };
                    for (kind, table) in target.iter_mut() {
                        if DEPENDENCY_TABLES.contains(&kind.get()) {
                            tables.extend(table.as_table_like_mut());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    tables
}

/// Replace a string value in place, keeping its surrounding formatting.
/// Returns false, changing nothing, when the item is not a string.
fn set_string(item: &mut Item, text: &str) -> bool {
    let Some(value) = item.as_value_mut().filter(|value| value.is_str()) else {
        return false;
    };
    let decor = value.decor().clone();
    *value = Value::from(text);
    *value.decor_mut() = decor;
    true
}

fn relative<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}
