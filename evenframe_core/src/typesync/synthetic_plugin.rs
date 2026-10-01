//! WASM plugin manager for synthetic-item plugins.
//!
//! This is a sibling of [`super::plugin::OutputRulePluginManager`]. The key
//! differences:
//!
//! - The required WASM export is `generate_items`, not `transform_type`.
//! - The plugin's *role* is to *add* new structs/enums/tables derived from the
//!   scanner results, not to override existing ones.

use crate::error::EvenframeError;
use std::collections::BTreeMap;
use std::path::Path;
use tracing::info;

use super::plugin_runtime::LoadedPlugin;
use super::synthetic_plugin_types::{SyntheticPluginInput, SyntheticPluginOutput};

pub struct SyntheticItemPluginManager {
    plugins: BTreeMap<String, LoadedPlugin>,
}

impl std::fmt::Debug for SyntheticItemPluginManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyntheticItemPluginManager")
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl SyntheticItemPluginManager {
    pub fn new(
        plugin_configs: &BTreeMap<String, crate::config::SyntheticItemPluginConfig>,
        project_root: &Path,
    ) -> Result<Self, EvenframeError> {
        let plugins = plugin_configs
            .iter()
            .map(|(name, config)| {
                let wasm_path = project_root.join(&config.path);
                info!(
                    "Loading synthetic-item plugin '{name}' from {}",
                    wasm_path.display()
                );
                let label = format!("Synthetic-item plugin '{name}'");
                let plugin = LoadedPlugin::load(&label, &wasm_path, "generate_items")?;
                Ok((name.clone(), plugin))
            })
            .collect::<Result<BTreeMap<_, _>, EvenframeError>>()?;
        info!("Loaded {} synthetic-item plugin(s)", plugins.len());
        Ok(Self { plugins })
    }

    /// The plugins' names, in the order they run.
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }

    /// Calls the `generate_items` entry point on a single loaded plugin.
    pub fn generate_items(
        &mut self,
        plugin_name: &str,
        input: &SyntheticPluginInput,
    ) -> Result<SyntheticPluginOutput, EvenframeError> {
        let plugin = self.plugins.get_mut(plugin_name).ok_or_else(|| {
            EvenframeError::plugin(format!("Synthetic-item plugin '{plugin_name}' not found"))
        })?;
        let input_json = serde_json::to_vec(input).map_err(|error| {
            EvenframeError::plugin(format!("Failed to serialize plugin input: {error}"))
        })?;
        let raw = plugin.call(&input_json)?;
        serde_json::from_str(&raw).map_err(|error| {
            EvenframeError::plugin(format!(
                "Synthetic-item plugin '{plugin_name}' returned invalid JSON: {error} (raw: {raw})"
            ))
        })
    }
}
