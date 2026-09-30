//! WASM plugin manager for mock data generation.

use crate::error::EvenframeError;
use std::collections::BTreeMap;
use std::path::Path;
use tracing::{debug, info};
use wasmtime::*;

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

/// Manages WASM plugin loading, caching, and invocation.
pub struct PluginManager {
    _engine: Engine,
    plugins: BTreeMap<String, MockPlugin>,
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginManager")
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
        let engine = Engine::default();
        let mut plugins = BTreeMap::new();

        for (name, config) in plugin_configs {
            let wasm_path = project_root.join(&config.path);
            if !wasm_path.exists() {
                return Err(EvenframeError::plugin(format!(
                    "Plugin '{}': WASM file not found at {}",
                    name,
                    wasm_path.display()
                )));
            }

            info!(
                "Loading WASM plugin '{}' from {}",
                name,
                wasm_path.display()
            );

            let module = Module::from_file(&engine, &wasm_path).map_err(|e| {
                EvenframeError::plugin(format!("Plugin '{}': failed to compile WASM: {}", name, e))
            })?;

            let mut store = Store::new(&engine, ());
            let linker = Linker::new(&engine);
            let instance = linker.instantiate(&mut store, &module).map_err(|e| {
                EvenframeError::plugin(format!("Plugin '{}': failed to instantiate: {}", name, e))
            })?;

            // Verify required exports
            let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
                EvenframeError::plugin(format!("Plugin '{}': missing 'memory' export", name))
            })?;

            // Verify alloc/dealloc exist
            instance
                .get_typed_func::<i32, i32>(&mut store, "alloc")
                .map_err(|_| {
                    EvenframeError::plugin(format!("Plugin '{}': missing 'alloc' export", name))
                })?;
            instance
                .get_typed_func::<(i32, i32), ()>(&mut store, "dealloc")
                .map_err(|_| {
                    EvenframeError::plugin(format!("Plugin '{}': missing 'dealloc' export", name))
                })?;

            // Mock data is generated field by field, so a plugin must export
            // `generate_field`; a table-level `generate_table` is not called.
            instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "generate_field")
                .map_err(|_| {
                    EvenframeError::plugin(format!(
                        "Plugin '{name}': mock-data plugins must export 'generate_field'"
                    ))
                })?;
            debug!("Plugin '{}' loaded", name);

            plugins.insert(
                name.clone(),
                MockPlugin {
                    runtime: LoadedPlugin {
                        store,
                        instance,
                        memory,
                    },
                    params: config.params.clone(),
                },
            );
        }

        info!("Loaded {} WASM plugin(s)", plugins.len());
        Ok(Self {
            _engine: engine,
            plugins,
        })
    }

    /// A field value from a named plugin, or `None` when the plugin skips
    /// the field and leaves it to the default generator.
    pub fn generate_field_value(
        &mut self,
        plugin_name: &str,
        input: &PluginFieldInput,
    ) -> Result<Option<String>, EvenframeError> {
        let plugin = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| EvenframeError::plugin(format!("Plugin '{}' not found", plugin_name)))?;

        let input_json = serde_json::to_vec(&WithParams {
            input,
            params: &plugin.params,
        })
        .map_err(|e| EvenframeError::plugin(format!("Failed to serialize input: {}", e)))?;

        let output_str = plugin
            .runtime
            .call_plugin_fn("generate_field", &input_json)?;

        let output: PluginFieldOutput = serde_json::from_str(&output_str).map_err(|e| {
            EvenframeError::plugin(format!(
                "Plugin '{}' returned invalid JSON: {} (raw: {})",
                plugin_name, e, output_str
            ))
        })?;

        match output.error.as_deref() {
            Some(SKIP) => return Ok(None),
            Some(err) => {
                return Err(EvenframeError::plugin(format!(
                    "Plugin '{}' error: {}",
                    plugin_name, err
                )));
            }
            None => {}
        }

        output.value.map(Some).ok_or_else(|| {
            EvenframeError::plugin(format!(
                "Plugin '{}' returned neither value nor error",
                plugin_name
            ))
        })
    }
}
