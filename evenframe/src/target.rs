//! What a command runs over: the one project its configuration describes, or
//! a workspace of projects.

use crate::scan_cache::build_and_record;
use evenframe_core::{
    config::{
        EvenframeConfig,
        workspace::{Project, Workspace},
    },
    error::Result,
    scan::ScanConfig,
    types::{AllConfigs, merge_foreign_types, merge_project_types},
};

/// The projects a command acts on, found from where it runs.
///
/// Configs load offline: a project's database settings are required only
/// when a command connects to it, by `load_connected_config`.
pub enum Target {
    Project(EvenframeConfig),
    Workspace(Workspace),
}

/// One project's configuration and the types its scan found.
pub struct Scanned<'a> {
    pub name: &'a str,
    pub config: &'a EvenframeConfig,
    pub types: AllConfigs,
}

impl Target {
    pub fn discover() -> Result<Self> {
        Ok(match Workspace::discover(false)? {
            Some(workspace) => Self::Workspace(workspace),
            None => Self::Project(EvenframeConfig::new_offline()?),
        })
    }

    /// The projects the run focuses on: the workspace root covers them all,
    /// a project directory only itself.
    pub fn focused(&self) -> Vec<(&str, &EvenframeConfig)> {
        match self {
            Self::Project(config) => vec![("", config)],
            Self::Workspace(workspace) => workspace.focused().map(named).collect(),
        }
    }

    /// Every project, focused or not.
    fn all(&self) -> Vec<(&str, &EvenframeConfig)> {
        match self {
            Self::Project(config) => vec![("", config)],
            Self::Workspace(workspace) => workspace.projects.iter().map(named).collect(),
        }
    }

    /// Scans every project with its own configuration.
    pub fn scan_all(&self) -> Result<Vec<Scanned<'_>>> {
        Self::scan(self.all())
    }

    /// Scans the focused projects with their own configurations.
    pub fn scan_focused(&self) -> Result<Vec<Scanned<'_>>> {
        Self::scan(self.focused())
    }

    fn scan<'a>(projects: Vec<(&'a str, &'a EvenframeConfig)>) -> Result<Vec<Scanned<'a>>> {
        projects
            .into_iter()
            .map(|(name, config)| {
                Ok(Scanned {
                    name,
                    config,
                    types: build_and_record(&ScanConfig::from_config(config))?,
                })
            })
            .collect()
    }

    /// The configuration typesync follows and the types it writes: a lone
    /// project's own, or for a workspace its config with every project's
    /// foreign types, over every project's types as one set.
    pub fn typesync_input(&self, scanned: &[Scanned<'_>]) -> Result<(EvenframeConfig, AllConfigs)> {
        let mut config = match self {
            Self::Project(config) => config.clone(),
            Self::Workspace(workspace) => workspace.config.clone(),
        };
        config.general.foreign_types = merge_foreign_types(
            scanned
                .iter()
                .map(|project| (project.name, &project.config.general.foreign_types)),
        )?;
        let types = merge_project_types(
            scanned
                .iter()
                .map(|project| (project.name, project.types.clone())),
        )?;
        Ok((config, types))
    }
}

fn named(project: &Project) -> (&str, &EvenframeConfig) {
    (project.name.as_str(), &project.config)
}
