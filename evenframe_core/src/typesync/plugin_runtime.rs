//! Shared WASM runtime used by the output-rule and synthetic-item plugin managers.
//!
//! Both plugin categories use the same pointer-length calling convention:
//!
//! - The plugin exports `alloc(size: i32) -> i32`, `dealloc(ptr: i32, len: i32)`,
//!   and a `memory` export.
//! - Each plugin function takes `(ptr: i32, len: i32) -> i64` where the returned
//!   i64 is a packed `(ptr << 32) | len` of the output bytes.
//!
//! `LoadedPlugin::call_plugin_fn` handles allocation, copying, the call, and
//! extraction of the returned bytes.

use crate::error::EvenframeError;
use wasmtime::{Instance, Memory, Store};

/// A single WASM plugin that has been instantiated and is ready to be called.
///
/// Both `OutputRulePluginManager` and `SyntheticItemPluginManager` hold one
/// of these per loaded plugin.
pub(crate) struct LoadedPlugin {
    pub store: Store<()>,
    pub instance: Instance,
    pub memory: Memory,
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

    /// Invokes `fn_name` on the plugin with a JSON input buffer and returns
    /// the raw UTF-8 output string.
    pub fn call_plugin_fn(
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
