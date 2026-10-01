//! WASM plugin manager for mock data generation.

use crate::error::EvenframeError;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};
use tracing::info;

use crate::typesync::plugin_runtime::LoadedPlugin;

use super::plugin_types::{PluginFieldInput, PluginFieldOutput};

/// A loaded mock-data plugin.
struct MockPlugin {
    runtime: LoadedPlugin,
    /// Config-supplied parameters forwarded to the plugin on every call.
    params: BTreeMap<String, String>,
}

/// Wrapper that attaches the plugin's config `params` to the serialized
/// input so plugins can vary output without a host ABI change per use case.
#[derive(serde::Serialize)]
struct WithParams<'a, T: serde::Serialize> {
    #[serde(flatten)]
    input: &'a T,
    params: &'a BTreeMap<String, String>,
}

/// The error a plugin returns to leave a field to the default generator.
const SKIP: &str = "skip";

/// How much a run has used its mock plugins: the field values asked of
/// them and the time those requests took, serialization included.
#[derive(Debug, Clone, Copy, Default)]
pub struct PluginUsage {
    pub calls: u64,
    pub time: Duration,
}

/// Manages WASM plugin loading, caching, and invocation.
pub struct PluginManager {
    plugins: BTreeMap<String, MockPlugin>,
    usage: PluginUsage,
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginManager")
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl PluginManager {
    /// Load all configured plugins from disk.
    pub fn new(
        plugin_configs: &BTreeMap<String, crate::schemasync::config::PluginConfig>,
        project_root: &Path,
    ) -> Result<Self, EvenframeError> {
        let plugins = plugin_configs
            .iter()
            .map(|(name, config)| {
                let wasm_path = project_root.join(&config.path);
                info!("Loading WASM plugin '{name}' from {}", wasm_path.display());
                // Mock data is generated field by field, so a plugin must
                // export `generate_field`.
                let runtime =
                    LoadedPlugin::load(&format!("Plugin '{name}'"), &wasm_path, "generate_field")?;
                let plugin = MockPlugin {
                    runtime,
                    params: config.params.clone(),
                };
                Ok((name.clone(), plugin))
            })
            .collect::<Result<BTreeMap<_, _>, EvenframeError>>()?;
        info!("Loaded {} WASM plugin(s)", plugins.len());
        Ok(Self {
            plugins,
            usage: PluginUsage::default(),
        })
    }

    /// What this run has asked of its plugins so far.
    pub fn usage(&self) -> PluginUsage {
        self.usage
    }

    /// A field value from a named plugin, or `None` when the plugin skips
    /// the field and leaves it to the default generator.
    pub fn generate_field_value(
        &mut self,
        plugin_name: &str,
        input: &PluginFieldInput,
    ) -> Result<Option<String>, EvenframeError> {
        let started = Instant::now();
        let plugin = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| EvenframeError::plugin(format!("Plugin '{plugin_name}' not found")))?;
        let input_json = serde_json::to_vec(&WithParams {
            input,
            params: &plugin.params,
        })
        .map_err(|error| {
            EvenframeError::plugin(format!("Failed to serialize plugin input: {error}"))
        })?;
        let raw = plugin.runtime.call(&input_json)?;
        let output: PluginFieldOutput = serde_json::from_str(&raw).map_err(|error| {
            EvenframeError::plugin(format!(
                "Plugin '{plugin_name}' returned invalid JSON: {error} (raw: {raw})"
            ))
        })?;
        self.usage.calls += 1;
        self.usage.time += started.elapsed();

        match output.error.as_deref() {
            Some(SKIP) => return Ok(None),
            Some(error) => {
                return Err(EvenframeError::plugin(format!(
                    "Plugin '{plugin_name}' error: {error}"
                )));
            }
            None => {}
        }
        output.value.map(Some).ok_or_else(|| {
            EvenframeError::plugin(format!(
                "Plugin '{plugin_name}' returned neither value nor error"
            ))
        })
    }
}
