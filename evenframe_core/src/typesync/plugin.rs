//! WASM plugin manager for output rule plugins.

use crate::error::EvenframeError;
use std::collections::BTreeMap;
use std::path::Path;
use tracing::{debug, info, warn};
use wasmtime::*;

use super::plugin_runtime::LoadedPlugin;
use super::plugin_types::{OutputRulePluginInput, OutputRulePluginOutput};

pub struct OutputRulePluginManager {
    _engine: Engine,
    plugin_names: Vec<String>,
    plugins: BTreeMap<String, LoadedPlugin>,
}

impl std::fmt::Debug for OutputRulePluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputRulePluginManager")
            .field("plugins", &self.plugin_names)
            .finish()
    }
}

impl OutputRulePluginManager {
    pub fn new(
        plugin_configs: &BTreeMap<String, crate::config::OutputRulePluginConfig>,
        project_root: &Path,
    ) -> Result<Self, EvenframeError> {
        let engine = Engine::default();
        let mut plugins = BTreeMap::new();
        let mut plugin_names = Vec::new();

        for (name, config) in plugin_configs {
            let wasm_path = project_root.join(&config.path);
            if !wasm_path.exists() {
                return Err(EvenframeError::plugin(format!(
                    "Output rule plugin '{}': WASM file not found at {}",
                    name,
                    wasm_path.display()
                )));
            }

            info!(
                "Loading output-rule plugin '{}' from {}",
                name,
                wasm_path.display()
            );

            let module = Module::from_file(&engine, &wasm_path).map_err(|e| {
                EvenframeError::plugin(format!(
                    "Output rule plugin '{}': failed to compile WASM: {}",
                    name, e
                ))
            })?;

            let mut store = Store::new(&engine, ());
            let linker = Linker::new(&engine);
            let instance = linker.instantiate(&mut store, &module).map_err(|e| {
                EvenframeError::plugin(format!(
                    "Output rule plugin '{}': failed to instantiate: {}",
                    name, e
                ))
            })?;

            let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
                EvenframeError::plugin(format!(
                    "Output rule plugin '{}': missing 'memory' export",
                    name
                ))
            })?;

            instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "transform_type")
                .map_err(|_| {
                    EvenframeError::plugin(format!(
                        "Output rule plugin '{}': missing 'transform_type' export",
                        name
                    ))
                })?;

            debug!("Output rule plugin '{}' loaded successfully", name);
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

        info!("Loaded {} output-rule plugin(s)", plugins.len());
        Ok(Self {
            _engine: engine,
            plugin_names,
            plugins,
        })
    }

    pub fn transform_type(
        &mut self,
        plugin_name: &str,
        input: &OutputRulePluginInput,
    ) -> Result<OutputRulePluginOutput, EvenframeError> {
        let plugin = self.plugins.get_mut(plugin_name).ok_or_else(|| {
            EvenframeError::plugin(format!("Output rule plugin '{}' not found", plugin_name))
        })?;

        let input_json = serde_json::to_vec(input)
            .map_err(|e| EvenframeError::plugin(format!("Failed to serialize input: {}", e)))?;

        let output_str = plugin.call_plugin_fn("transform_type", &input_json)?;

        let output: OutputRulePluginOutput = serde_json::from_str(&output_str).map_err(|e| {
            EvenframeError::plugin(format!(
                "Output rule plugin '{}' returned invalid JSON: {} (raw: {})",
                plugin_name, e, output_str
            ))
        })?;

        if let Some(ref err) = output.error {
            warn!("Output rule plugin '{}' error: {}", plugin_name, err);
        }

        Ok(output)
    }

    pub fn plugin_names(&self) -> &[String] {
        &self.plugin_names
    }
}
