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
use tracing::{debug, info, warn};
use wasmtime::*;

use super::plugin_runtime::LoadedPlugin;
use super::synthetic_plugin_types::{SyntheticPluginInput, SyntheticPluginOutput};

pub struct SyntheticItemPluginManager {
    _engine: Engine,
    plugin_names: Vec<String>,
    plugins: BTreeMap<String, LoadedPlugin>,
}

impl std::fmt::Debug for SyntheticItemPluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyntheticItemPluginManager")
            .field("plugins", &self.plugin_names)
            .finish()
    }
}

impl SyntheticItemPluginManager {
    pub fn new(
        plugin_configs: &BTreeMap<String, crate::config::SyntheticItemPluginConfig>,
        project_root: &Path,
    ) -> Result<Self, EvenframeError> {
        let engine = Engine::default();
        let mut plugins = BTreeMap::new();
        let mut plugin_names = Vec::new();

        for (name, config) in plugin_configs {
            let wasm_path = project_root.join(&config.path);
            if !wasm_path.exists() {
                return Err(EvenframeError::plugin(format!(
                    "Synthetic-item plugin '{}': WASM file not found at {}",
                    name,
                    wasm_path.display()
                )));
            }

            info!(
                "Loading synthetic-item plugin '{}' from {}",
                name,
                wasm_path.display()
            );

            let module = Module::from_file(&engine, &wasm_path).map_err(|e| {
                EvenframeError::plugin(format!(
                    "Synthetic-item plugin '{}': failed to compile WASM: {}",
                    name, e
                ))
            })?;

            let mut store = Store::new(&engine, ());
            let linker = Linker::new(&engine);
            let instance = linker.instantiate(&mut store, &module).map_err(|e| {
                EvenframeError::plugin(format!(
                    "Synthetic-item plugin '{}': failed to instantiate: {}",
                    name, e
                ))
            })?;

            let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
                EvenframeError::plugin(format!(
                    "Synthetic-item plugin '{}': missing 'memory' export",
                    name
                ))
            })?;

            instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "generate_items")
                .map_err(|_| {
                    EvenframeError::plugin(format!(
                        "Synthetic-item plugin '{}': missing 'generate_items' export",
                        name
                    ))
                })?;

            debug!("Synthetic-item plugin '{}' loaded successfully", name);
            plugin_names.push(name.clone());
            plugins.insert(
                name.clone(),
                LoadedPlugin {
                    store,
                    instance,
                    memory,
                },
            );
        }

        info!("Loaded {} synthetic-item plugin(s)", plugins.len());
        Ok(Self {
            _engine: engine,
            plugin_names,
            plugins,
        })
    }

    /// Calls the `generate_items` entry point on a single loaded plugin.
    pub fn generate_items(
        &mut self,
        plugin_name: &str,
        input: &SyntheticPluginInput,
    ) -> Result<SyntheticPluginOutput, EvenframeError> {
        let plugin = self.plugins.get_mut(plugin_name).ok_or_else(|| {
            EvenframeError::plugin(format!("Synthetic-item plugin '{}' not found", plugin_name))
        })?;

        let input_json = serde_json::to_vec(input)
            .map_err(|e| EvenframeError::plugin(format!("Failed to serialize input: {}", e)))?;

        let output_str = plugin.call_plugin_fn("generate_items", &input_json)?;

        let output: SyntheticPluginOutput = serde_json::from_str(&output_str).map_err(|e| {
            EvenframeError::plugin(format!(
                "Synthetic-item plugin '{}' returned invalid JSON: {} (raw: {})",
                plugin_name, e, output_str
            ))
        })?;

        if let Some(ref err) = output.error {
            warn!("Synthetic-item plugin '{}' error: {}", plugin_name, err);
        }

        Ok(output)
    }

    pub fn plugin_names(&self) -> &[String] {
        &self.plugin_names
    }
}
