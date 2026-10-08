//! A workspace is a config that names its projects in `general.projects`.
//!
//! Each project is scanned with its own configuration (the workspace config
//! merged under the project's), so its plugins apply and its schemasync
//! reaches its own database. Typesync then writes one output for every
//! project together, so a type two projects share is generated once.

use super::{EvenframeConfig, chain};
use crate::error::{EvenframeError, Result};
use std::path::{Path, PathBuf};

/// One project of a workspace.
#[derive(Debug, Clone)]
pub struct Project {
    /// The project's directory as the workspace config names it.
    pub name: String,
    pub config: EvenframeConfig,
}

/// Which projects a run in a workspace acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    /// The run started at the workspace root: every project.
    All,
    /// The run started inside this project.
    Project(String),
}

/// A workspace, found from where a run starts.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The workspace config itself, whose typesync settings the unified
    /// output follows.
    pub config: EvenframeConfig,
    pub projects: Vec<Project>,
    pub focus: Focus,
}

impl Workspace {
    /// The workspace the configuration chain ending at `nearest` belongs to,
    /// or `None` when no config in the chain declares `projects`.
    pub fn from_nearest(nearest: &Path, require_connection_env: bool) -> Result<Option<Self>> {
        let chain = chain::chain_ending_at(nearest);
        let mut declaring = Vec::new();
        for path in &chain {
            let link = chain::Link::read(path)?;
            if let Some(projects) = link.projects() {
                declaring.push((path.clone(), projects));
            }
        }
        let (root_config_path, names) = match declaring.as_slice() {
            [] => return Ok(None),
            [only] => only.clone(),
            [outer, inner, ..] => {
                return Err(EvenframeError::config(format!(
                    "{} declares `projects` inside the workspace of {}; a workspace cannot \
                     contain another",
                    inner.0.display(),
                    outer.0.display()
                )));
            }
        };
        let root_dir = EvenframeConfig::project_root_of(&root_config_path).to_path_buf();
        let config = EvenframeConfig::load_from(root_config_path.clone(), require_connection_env)?;

        // A `--config` file in a project's directory stands in for that
        // project's own config.
        let nearest_dir: PathBuf = EvenframeConfig::project_root_of(nearest)
            .components()
            .collect();
        let mut projects: Vec<Project> = Vec::new();
        for name in names {
            // Without `.` components, so `./backend` and `backend` are one project.
            let dir: PathBuf = root_dir.join(&name).components().collect();
            let project_config = if dir == nearest_dir {
                nearest.to_path_buf()
            } else if let Some(own) = chain::config_in(&dir) {
                own
            } else {
                return Err(EvenframeError::config(format!(
                    "{} names the project `{name}`, but {} has no .evenframe/config.toml or \
                     evenframe.toml",
                    root_config_path.display(),
                    dir.display()
                )));
            };
            if project_config == root_config_path {
                return Err(EvenframeError::config(format!(
                    "{} names its own directory as a project",
                    root_config_path.display()
                )));
            }
            if let Some(nested) = chain::Link::read(&project_config)?.projects() {
                return Err(EvenframeError::config(format!(
                    "{} declares `projects` ({}) inside the workspace of {}; a workspace cannot \
                     contain another",
                    project_config.display(),
                    nested.join(", "),
                    root_config_path.display()
                )));
            }
            if let Some(duplicate) = projects
                .iter()
                .find(|project| project.config.config_file_path == project_config)
            {
                return Err(EvenframeError::config(format!(
                    "{} names the project at {} twice, as `{}` and `{name}`",
                    root_config_path.display(),
                    dir.display(),
                    duplicate.name
                )));
            }
            projects.push(Project {
                name,
                config: EvenframeConfig::load_from(project_config, require_connection_env)?,
            });
        }

        let focus = if nearest == root_config_path {
            Focus::All
        } else if let Some(project) = projects
            .iter()
            .find(|project| project.config.config_file_path == nearest)
        {
            Focus::Project(project.name.clone())
        } else {
            return Err(EvenframeError::config(format!(
                "{} is inside the workspace of {} but is not one of its projects; add its \
                 directory to `projects` there",
                nearest.display(),
                root_config_path.display()
            )));
        };
        Ok(Some(Self {
            config,
            projects,
            focus,
        }))
    }

    /// The workspace the current directory (or `--config`) belongs to.
    pub fn discover(require_connection_env: bool) -> Result<Option<Self>> {
        Self::from_nearest(
            &EvenframeConfig::find_config_file()?,
            require_connection_env,
        )
    }

    /// The projects the focus covers.
    pub fn focused(&self) -> impl Iterator<Item = &Project> {
        self.projects.iter().filter(|project| match &self.focus {
            Focus::All => true,
            Focus::Project(name) => *name == project.name,
        })
    }

    /// The workspace root directory.
    pub fn root(&self) -> PathBuf {
        self.config.project_root().to_path_buf()
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
