//! WASM plugin manager for mock data generation.

use crate::error::EvenframeError;
use std::collections::BTreeMap;
use std::path::Path;
use tracing::{debug, info};
use wasmtime::*;

use super::plugin_types::{
    PluginFieldInput, PluginFieldOutput, PluginTableInput, PluginTableOutput,
};

/// A loaded and instantiated WASM plugin.
struct LoadedPlugin {
    store: Store<()>,
    instance: Instance,
    memory: Memory,
}

impl LoadedPlugin {
    /// Get the `alloc` export.
    fn alloc(&mut self, size: i32) -> Result<i32, EvenframeError> {
        let func = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, "alloc")
            .map_err(|e| EvenframeError::plugin(format!("Missing 'alloc' export: {}", e)))?;
        func.call(&mut self.store, size)
            .map_err(|e| EvenframeError::plugin(format!("alloc failed: {}", e)))
    }

    /// Get the `dealloc` export.
    fn dealloc(&mut self, ptr: i32, len: i32) -> Result<(), EvenframeError> {
        let func = self
            .instance
            .get_typed_func::<(i32, i32), ()>(&mut self.store, "dealloc")
            .map_err(|e| EvenframeError::plugin(format!("Missing 'dealloc' export: {}", e)))?;
        func.call(&mut self.store, (ptr, len))
            .map_err(|e| EvenframeError::plugin(format!("dealloc failed: {}", e)))
    }

    /// Write input bytes into WASM memory and call a function, returning the output string.
    fn call_plugin_fn(
        &mut self,
        fn_name: &str,
        input_json: &[u8],
    ) -> Result<String, EvenframeError> {
        // Allocate space in WASM memory for the input
        let input_len = input_json.len() as i32;
        let input_ptr = self.alloc(input_len)?;

        // Write input bytes into WASM linear memory
        let mem_data = self.memory.data_mut(&mut self.store);
        let start = input_ptr as usize;
        let end = start + input_json.len();
        if end > mem_data.len() {
            return Err(EvenframeError::plugin(format!(
                "WASM memory too small: need {} bytes at offset {}, have {}",
                input_json.len(),
                start,
                mem_data.len()
            )));
        }
        mem_data[start..end].copy_from_slice(input_json);

        // Call the plugin function
        let func = self
            .instance
            .get_typed_func::<(i32, i32), i64>(&mut self.store, fn_name)
            .map_err(|e| EvenframeError::plugin(format!("Missing '{}' export: {}", fn_name, e)))?;

        let packed = func
            .call(&mut self.store, (input_ptr, input_len))
            .map_err(|e| {
                EvenframeError::plugin(format!("Plugin function '{}' trapped: {}", fn_name, e))
            })?;

        // Unpack pointer and length from i64
        let out_ptr = (packed >> 32) as i32;
        let out_len = (packed & 0xFFFF_FFFF) as i32;

        // Read output from WASM memory
        let mem_data = self.memory.data(&self.store);
        let out_start = out_ptr as usize;
        let out_end = out_start + out_len as usize;
        if out_end > mem_data.len() {
            return Err(EvenframeError::plugin(format!(
                "Plugin returned out-of-bounds pointer: {}+{} > {}",
                out_start,
                out_len,
                mem_data.len()
            )));
        }
        let output_bytes = mem_data[out_start..out_end].to_vec();

        // Deallocate the output in WASM
        let _ = self.dealloc(out_ptr, out_len);

        String::from_utf8(output_bytes)
            .map_err(|e| EvenframeError::plugin(format!("Plugin returned invalid UTF-8: {}", e)))
    }
}

/// Manages WASM plugin loading, caching, and invocation.
pub struct PluginManager {
    _engine: Engine,
    plugins: BTreeMap<String, LoadedPlugin>,
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

            // Verify at least one generation function exists
            let has_field = instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "generate_field")
                .is_ok();
            let has_table = instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "generate_table")
                .is_ok();
            if !has_field && !has_table {
                return Err(EvenframeError::plugin(format!(
                    "Plugin '{}': exports neither 'generate_field' nor 'generate_table'",
                    name
                )));
            }

            debug!(
                "Plugin '{}' loaded: generate_field={}, generate_table={}",
                name, has_field, has_table
            );

            plugins.insert(
                name.clone(),
                LoadedPlugin {
                    store,
                    instance,
                    memory,
                },
            );
        }

        info!("Loaded {} WASM plugin(s)", plugins.len());
        Ok(Self {
            _engine: engine,
            plugins,
        })
    }

    /// Generate a field value using a named plugin.
    pub fn generate_field_value(
        &mut self,
        plugin_name: &str,
        input: &PluginFieldInput,
    ) -> Result<String, EvenframeError> {
        let plugin = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| EvenframeError::plugin(format!("Plugin '{}' not found", plugin_name)))?;

        let input_json = serde_json::to_vec(input)
            .map_err(|e| EvenframeError::plugin(format!("Failed to serialize input: {}", e)))?;

        let output_str = plugin.call_plugin_fn("generate_field", &input_json)?;

        let output: PluginFieldOutput = serde_json::from_str(&output_str).map_err(|e| {
            EvenframeError::plugin(format!(
                "Plugin '{}' returned invalid JSON: {} (raw: {})",
                plugin_name, e, output_str
            ))
        })?;

        if let Some(err) = output.error {
            return Err(EvenframeError::plugin(format!(
                "Plugin '{}' error: {}",
                plugin_name, err
            )));
        }

        output.value.ok_or_else(|| {
            EvenframeError::plugin(format!(
                "Plugin '{}' returned neither value nor error",
                plugin_name
            ))
        })
    }

    /// Generate all field values for a table record using a named plugin.
    pub fn generate_table_values(
        &mut self,
        plugin_name: &str,
        input: &PluginTableInput,
    ) -> Result<BTreeMap<String, String>, EvenframeError> {
        let plugin = self
            .plugins
            .get_mut(plugin_name)
            .ok_or_else(|| EvenframeError::plugin(format!("Plugin '{}' not found", plugin_name)))?;

        let input_json = serde_json::to_vec(input)
            .map_err(|e| EvenframeError::plugin(format!("Failed to serialize input: {}", e)))?;

        let output_str = plugin.call_plugin_fn("generate_table", &input_json)?;

        let output: PluginTableOutput = serde_json::from_str(&output_str).map_err(|e| {
            EvenframeError::plugin(format!(
                "Plugin '{}' returned invalid JSON: {} (raw: {})",
                plugin_name, e, output_str
            ))
        })?;

        if let Some(err) = output.error {
            return Err(EvenframeError::plugin(format!(
                "Plugin '{}' error: {}",
                plugin_name, err
            )));
        }

        output.fields.ok_or_else(|| {
            EvenframeError::plugin(format!(
                "Plugin '{}' returned neither fields nor error",
                plugin_name
            ))
        })
    }
}
