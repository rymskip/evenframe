//! WASM plugin manager for output rule plugins.

use crate::error::EvenframeError;
use std::collections::BTreeMap;
use std::path::Path;
use tracing::info;

use super::plugin_runtime::LoadedPlugin;
use super::plugin_types::{OutputRulePluginInput, OutputRulePluginOutput};

pub struct OutputRulePluginManager {
    plugins: BTreeMap<String, LoadedPlugin>,
}

impl std::fmt::Debug for OutputRulePluginManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutputRulePluginManager")
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl OutputRulePluginManager {
    pub fn new(
        plugin_configs: &BTreeMap<String, crate::config::OutputRulePluginConfig>,
        project_root: &Path,
    ) -> Result<Self, EvenframeError> {
        let plugins = plugin_configs
            .iter()
            .map(|(name, config)| {
                let wasm_path = project_root.join(&config.path);
                info!(
                    "Loading output-rule plugin '{name}' from {}",
                    wasm_path.display()
                );
                let label = format!("Output rule plugin '{name}'");
                let plugin = LoadedPlugin::load(&label, &wasm_path, "transform_type")?;
                Ok((name.clone(), plugin))
            })
            .collect::<Result<BTreeMap<_, _>, EvenframeError>>()?;
        info!("Loaded {} output-rule plugin(s)", plugins.len());
        Ok(Self { plugins })
    }

    /// Every plugin's overrides for one type, in plugin order. The input is
    /// serialized once for all of them. The run stops at the first plugin
    /// that reports an error, since the type is rejected there.
    pub fn transform(
        &mut self,
        input: &OutputRulePluginInput,
    ) -> Result<Vec<(String, OutputRulePluginOutput)>, EvenframeError> {
        let input_json = serde_json::to_vec(input).map_err(|error| {
            EvenframeError::plugin(format!("Failed to serialize plugin input: {error}"))
        })?;
        let mut outputs = Vec::with_capacity(self.plugins.len());
        for (name, plugin) in &mut self.plugins {
            let raw = plugin.call(&input_json).map_err(|error| {
                EvenframeError::plugin(format!("Output rule plugin '{name}': {error}"))
            })?;
            let output: OutputRulePluginOutput = serde_json::from_str(&raw).map_err(|error| {
                EvenframeError::plugin(format!(
                    "Output rule plugin '{name}' returned invalid JSON: {error} (raw: {raw})"
                ))
            })?;
            let reported_error = output.error.is_some();
            outputs.push((name.clone(), output));
            if reported_error {
                break;
            }
        }
        Ok(outputs)
    }
}
