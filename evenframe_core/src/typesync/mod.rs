// Always compiled (core typesync)
pub mod arktype;
pub mod config;
pub mod doc_comment;
pub mod effect;
pub mod file_grouping;
pub mod import_resolver;
pub mod plugin_types;

#[cfg(feature = "wasm-plugins")]
pub mod plugin;

#[cfg(all(test, feature = "macroforge"))]
pub mod testing;

// Feature-gated parsers
#[cfg(feature = "flatbuffers")]
pub mod flatbuffers;
#[cfg(feature = "flatbuffers")]
pub mod flatbuffers_parser;

#[cfg(feature = "protobuf")]
pub mod protobuf;
#[cfg(feature = "protobuf")]
pub mod protobuf_parser;

#[cfg(feature = "macroforge")]
pub mod effect_template;
#[cfg(feature = "macroforge")]
pub mod macroforge;
