//! WASM plugin manager for type-transform plugins.
//!
//! Type-transform plugins receive full struct/enum context and return
//! modifications (type overrides, skipped fields, extra imports, etc.).

use crate::error::EvenframeError;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info, warn};
use wasmtime::*;

use super::plugin_types::{TypeOverrides, TypePluginInput, TypePluginOutput};

/// A loaded and instantiated WASM type-transform plugin.
struct LoadedPlugin {
    store: Store<()>,
    instance: Instance,
    memory: Memory,
}

impl LoadedPlugin {
    fn alloc(&mut self, size: i32) -> Result<i32, EvenframeError> {
        let func = self
            .instance
            .get_typed_func::<i32, i32>(&mut self.store, "alloc")
            .map_err(|e| EvenframeError::plugin(format!("Missing 'alloc' export: {}", e)))?;
        func.call(&mut self.store, size)
            .map_err(|e| EvenframeError::plugin(format!("alloc failed: {}", e)))
    }

    fn dealloc(&mut self, ptr: i32, len: i32) -> Result<(), EvenframeError> {
        let func = self
            .instance
            .get_typed_func::<(i32, i32), ()>(&mut self.store, "dealloc")
            .map_err(|e| EvenframeError::plugin(format!("Missing 'dealloc' export: {}", e)))?;
        func.call(&mut self.store, (ptr, len))
            .map_err(|e| EvenframeError::plugin(format!("dealloc failed: {}", e)))
    }

    fn call_plugin_fn(
        &mut self,
        fn_name: &str,
        input_json: &[u8],
    ) -> Result<String, EvenframeError> {
        let input_len = input_json.len() as i32;
        let input_ptr = self.alloc(input_len)?;

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

        let func = self
            .instance
            .get_typed_func::<(i32, i32), i64>(&mut self.store, fn_name)
            .map_err(|e| EvenframeError::plugin(format!("Missing '{}' export: {}", fn_name, e)))?;

        let packed = func
            .call(&mut self.store, (input_ptr, input_len))
            .map_err(|e| {
                EvenframeError::plugin(format!("Plugin function '{}' trapped: {}", fn_name, e))
            })?;

        let out_ptr = (packed >> 32) as i32;
        let out_len = (packed & 0xFFFF_FFFF) as i32;

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

        let _ = self.dealloc(out_ptr, out_len);

        String::from_utf8(output_bytes)
            .map_err(|e| EvenframeError::plugin(format!("Plugin returned invalid UTF-8: {}", e)))
    }
}

/// Manages WASM type-transform plugin loading and invocation.
pub struct TypePluginManager {
    _engine: Engine,
    plugin_names: Vec<String>,
    plugins: HashMap<String, LoadedPlugin>,
}

impl std::fmt::Debug for TypePluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypePluginManager")
            .field("plugins", &self.plugin_names)
            .finish()
    }
}

impl TypePluginManager {
    /// Load all configured type-transform plugins.
    pub fn new(
        plugin_configs: &HashMap<String, crate::config::TypePluginConfig>,
        project_root: &Path,
    ) -> Result<Self, EvenframeError> {
        let engine = Engine::default();
        let mut plugins = HashMap::new();
        let mut plugin_names = Vec::new();

        for (name, config) in plugin_configs {
            let wasm_path = project_root.join(&config.path);
            if !wasm_path.exists() {
                return Err(EvenframeError::plugin(format!(
                    "Type plugin '{}': WASM file not found at {}",
                    name,
                    wasm_path.display()
                )));
            }

            info!(
                "Loading type-transform plugin '{}' from {}",
                name,
                wasm_path.display()
            );

            let module = Module::from_file(&engine, &wasm_path).map_err(|e| {
                EvenframeError::plugin(format!(
                    "Type plugin '{}': failed to compile WASM: {}",
                    name, e
                ))
            })?;

            let mut store = Store::new(&engine, ());
            let linker = Linker::new(&engine);
            let instance = linker.instantiate(&mut store, &module).map_err(|e| {
                EvenframeError::plugin(format!(
                    "Type plugin '{}': failed to instantiate: {}",
                    name, e
                ))
            })?;

            let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
                EvenframeError::plugin(format!("Type plugin '{}': missing 'memory' export", name))
            })?;

            // Verify alloc/dealloc
            instance
                .get_typed_func::<i32, i32>(&mut store, "alloc")
                .map_err(|_| {
                    EvenframeError::plugin(format!(
                        "Type plugin '{}': missing 'alloc' export",
                        name
                    ))
                })?;
            instance
                .get_typed_func::<(i32, i32), ()>(&mut store, "dealloc")
                .map_err(|_| {
                    EvenframeError::plugin(format!(
                        "Type plugin '{}': missing 'dealloc' export",
                        name
                    ))
                })?;

            // Verify transform_type export
            instance
                .get_typed_func::<(i32, i32), i64>(&mut store, "transform_type")
                .map_err(|_| {
                    EvenframeError::plugin(format!(
                        "Type plugin '{}': missing 'transform_type' export",
                        name
                    ))
                })?;

            debug!("Type plugin '{}' loaded successfully", name);

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

        info!("Loaded {} type-transform plugin(s)", plugins.len());
        Ok(Self {
            _engine: engine,
            plugin_names,
            plugins,
        })
    }

    /// Call a single plugin's transform_type function.
    pub fn transform_type(
        &mut self,
        plugin_name: &str,
        input: &TypePluginInput,
    ) -> Result<TypePluginOutput, EvenframeError> {
        let plugin = self.plugins.get_mut(plugin_name).ok_or_else(|| {
            EvenframeError::plugin(format!("Type plugin '{}' not found", plugin_name))
        })?;

        let input_json = serde_json::to_vec(input)
            .map_err(|e| EvenframeError::plugin(format!("Failed to serialize input: {}", e)))?;

        let output_str = plugin.call_plugin_fn("transform_type", &input_json)?;

        let output: TypePluginOutput = serde_json::from_str(&output_str).map_err(|e| {
            EvenframeError::plugin(format!(
                "Type plugin '{}' returned invalid JSON: {} (raw: {})",
                plugin_name, e, output_str
            ))
        })?;

        if let Some(ref err) = output.error {
            warn!("Type plugin '{}' error: {}", plugin_name, err);
        }

        Ok(output)
    }

    /// Run all plugins for a given type and generator, collecting overrides.
    ///
    /// Plugins execute in config order. Last plugin wins for conflicting overrides.
    pub fn compute_overrides(&mut self, input: &TypePluginInput) -> TypeOverrides {
        let mut overrides = TypeOverrides::default();
        let type_name = &input.type_name;

        for plugin_name in self.plugin_names.clone() {
            match self.transform_type(&plugin_name, input) {
                Ok(output) => {
                    if output.error.is_some() {
                        continue; // Skip plugins that report errors
                    }

                    for (field, ty) in output.field_type_overrides {
                        overrides.field_types.insert((type_name.clone(), field), ty);
                    }

                    if !output.skip_fields.is_empty() {
                        overrides
                            .skip_fields
                            .entry(type_name.clone())
                            .or_default()
                            .extend(output.skip_fields);
                    }

                    overrides.extra_imports.extend(output.extra_imports);

                    for (field, annotations) in output.field_annotations {
                        overrides
                            .field_annotations
                            .entry((type_name.clone(), field))
                            .or_default()
                            .extend(annotations);
                    }

                    if let Some(name_override) = output.type_name_override {
                        overrides
                            .type_name_overrides
                            .insert(type_name.clone(), name_override);
                    }
                }
                Err(e) => {
                    warn!(
                        "Type plugin '{}' failed for type '{}': {}",
                        plugin_name, type_name, e
                    );
                }
            }
        }

        overrides
    }

    /// Returns the ordered list of plugin names.
    pub fn plugin_names(&self) -> &[String] {
        &self.plugin_names
    }
}
