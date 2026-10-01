//! Shared WASM runtime used by the output-rule, synthetic-item and mock-data
//! plugin managers.
//!
//! All plugin categories use the same pointer-length calling convention:
//!
//! - The plugin exports `alloc(size: i32) -> i32`, `dealloc(ptr: i32, len: i32)`,
//!   and a `memory` export.
//! - Each plugin function takes `(ptr: i32, len: i32) -> i64` where the returned
//!   i64 is a packed `(ptr << 32) | len` of the output bytes.
//!
//! [`LoadedPlugin::call`] handles allocation, copying, the call, and
//! extraction of the returned bytes.

use crate::error::EvenframeError;
use std::path::Path;
use std::sync::LazyLock;
use tracing::warn;
use wasmtime::{Cache, CacheConfig, Config, Engine, Linker, Memory, Module, Store, TypedFunc};

/// The engine every plugin compiles on: one per process, with wasmtime's
/// compilation cache, so a plugin that has not changed is not recompiled on
/// every run.
static ENGINE: LazyLock<Result<Engine, String>> = LazyLock::new(|| {
    let mut config = Config::new();
    match Cache::new(CacheConfig::new()) {
        Ok(cache) => {
            config.cache(Some(cache));
        }
        Err(error) => warn!("Compiling WASM plugins without wasmtime's cache: {error}"),
    }
    Engine::new(&config).map_err(|error| error.to_string())
});

fn engine() -> Result<&'static Engine, EvenframeError> {
    ENGINE.as_ref().map_err(|error| {
        EvenframeError::plugin(format!("Failed to create the WASM engine: {error}"))
    })
}

/// A WASM plugin, instantiated with the exports every call needs.
pub(crate) struct LoadedPlugin {
    store: Store<()>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    dealloc: TypedFunc<(i32, i32), ()>,
    entry: TypedFunc<(i32, i32), i64>,
}

impl LoadedPlugin {
    /// Compiles and instantiates the plugin at `wasm_path` and resolves its
    /// `entry` export. `label` names the plugin in errors, such as
    /// "Output rule plugin 'stamp'".
    pub fn load(label: &str, wasm_path: &Path, entry: &str) -> Result<Self, EvenframeError> {
        if !wasm_path.exists() {
            return Err(EvenframeError::plugin(format!(
                "{label}: WASM file not found at {}",
                wasm_path.display()
            )));
        }
        let engine = engine()?;
        let module = Module::from_file(engine, wasm_path).map_err(|error| {
            EvenframeError::plugin(format!("{label}: failed to compile WASM: {error}"))
        })?;
        let mut store = Store::new(engine, ());
        let instance = Linker::new(engine)
            .instantiate(&mut store, &module)
            .map_err(|error| {
                EvenframeError::plugin(format!("{label}: failed to instantiate: {error}"))
            })?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| EvenframeError::plugin(format!("{label}: missing 'memory' export")))?;
        let missing = |export: &str, error: wasmtime::Error| {
            EvenframeError::plugin(format!("{label}: missing '{export}' export: {error}"))
        };
        let alloc = instance
            .get_typed_func(&mut store, "alloc")
            .map_err(|error| missing("alloc", error))?;
        let dealloc = instance
            .get_typed_func(&mut store, "dealloc")
            .map_err(|error| missing("dealloc", error))?;
        let entry = instance
            .get_typed_func(&mut store, entry)
            .map_err(|error| missing(entry, error))?;
        Ok(Self {
            store,
            memory,
            alloc,
            dealloc,
            entry,
        })
    }

    /// Calls the plugin's entry point with `input` and returns its UTF-8
    /// output.
    pub fn call(&mut self, input: &[u8]) -> Result<String, EvenframeError> {
        let input_len = i32::try_from(input.len()).map_err(|_| {
            EvenframeError::plugin(format!(
                "Plugin input of {} bytes is over the 2 GiB a WASM call can take",
                input.len()
            ))
        })?;
        let input_ptr = self
            .alloc
            .call(&mut self.store, input_len)
            .map_err(|error| EvenframeError::plugin(format!("alloc failed: {error}")))?;
        let input_range = guest_range(input_ptr.cast_unsigned(), input_len.cast_unsigned())?;
        let memory = self.memory.data_mut(&mut self.store);
        let memory_len = memory.len();
        let input_slot = memory.get_mut(input_range.clone()).ok_or_else(|| {
            EvenframeError::plugin(format!(
                "WASM memory too small: need bytes {input_range:?}, have {memory_len}"
            ))
        })?;
        input_slot.copy_from_slice(input);

        let packed = self
            .entry
            .call(&mut self.store, (input_ptr, input_len))
            .map_err(|error| EvenframeError::plugin(format!("Plugin call trapped: {error}")))?;
        // The guest only borrows the input, so the host frees it; otherwise
        // guest memory grows with every call.
        self.free(input_ptr, input_len)?;

        // The high half is the output's address and the low half its length.
        let packed = packed.cast_unsigned();
        let output_ptr = (packed >> 32) as u32;
        let output_len = packed as u32;
        let output_range = guest_range(output_ptr, output_len)?;
        let memory = self.memory.data(&self.store);
        let output = memory
            .get(output_range.clone())
            .ok_or_else(|| {
                EvenframeError::plugin(format!(
                    "Plugin returned bytes {output_range:?} past its memory of {}",
                    memory.len()
                ))
            })?
            .to_vec();
        self.free(output_ptr.cast_signed(), output_len.cast_signed())?;

        String::from_utf8(output).map_err(|error| {
            EvenframeError::plugin(format!("Plugin returned invalid UTF-8: {error}"))
        })
    }

    fn free(&mut self, ptr: i32, len: i32) -> Result<(), EvenframeError> {
        self.dealloc
            .call(&mut self.store, (ptr, len))
            .map_err(|error| EvenframeError::plugin(format!("dealloc failed: {error}")))
    }
}

/// The host byte range of a guest address and length, which the calling
/// convention passes as `i32` but the guest means as `u32`.
fn guest_range(ptr: u32, len: u32) -> Result<std::ops::Range<usize>, EvenframeError> {
    let (Ok(start), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return Err(EvenframeError::plugin(format!(
            "Guest range {ptr}+{len} does not fit this host"
        )));
    };
    Ok(start..start + len)
}
