// Always compiled: EvenframeConfig and the mock plugins use these.
pub mod config;
pub mod naming;
pub mod plugin_types;

#[cfg(feature = "arktype")]
pub mod arktype;
#[cfg(feature = "typesync")]
pub mod checks;
#[cfg(feature = "typesync")]
#[cfg(feature = "typesync")]
pub mod doc_comment;
#[cfg(feature = "effect")]
pub mod effect;
#[cfg(feature = "typesync")]
pub mod file_grouping;
#[cfg(feature = "typesync")]
pub mod foreign_ts;
#[cfg(feature = "typesync")]
pub mod import_resolver;
#[cfg(feature = "typesync")]
pub mod js_checks;
#[cfg(feature = "typesync")]
pub mod map_key;
#[cfg(feature = "typesync")]
pub mod output;
pub mod struct_variants;
#[cfg(feature = "typesync")]
pub mod type_index;

#[cfg(feature = "wasm-plugins")]
pub mod plugin;
#[cfg(feature = "wasm-plugins")]
pub(crate) mod plugin_runtime;
#[cfg(feature = "wasm-plugins")]
pub mod synthetic_plugin;
#[cfg(feature = "wasm-plugins")]
pub mod synthetic_plugin_types;

#[cfg(all(test, feature = "macroforge"))]
pub mod testing;

// Feature-gated parsers
#[cfg(feature = "flatbuffers")]
pub mod flatbuffers;

#[cfg(feature = "protobuf")]
pub mod protobuf;
#[cfg(feature = "protobuf")]
pub mod protobuf_rules;

#[cfg(feature = "macroforge")]
pub mod macroforge;
