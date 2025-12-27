//! Protocol Buffers schema parser for evenframe.
//!
//! This module provides the ability to parse Protocol Buffers schema files (.proto)
//! and convert them to evenframe's internal type representations.
//!
//! Uses the `protox` crate for pure Rust proto parsing (no protoc binary required).
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use evenframe_core::typesync::protobuf_parser::parse_protobuf_files;
//!
//! let result = parse_protobuf_files(&[Path::new("schema.proto")], &[]).unwrap();
//! println!("Found {} messages", result.structs.len());
//! println!("Found {} enums", result.enums.len());
//! ```
//!
//! # Validation Support
//!
//! Validation annotations can be added using custom options or comments:
//!
//! ```proto
//! message User {
//!     // @validate: email
//!     string email = 1;
//!
//!     // @validate: min(0), max(150)
//!     int32 age = 2;
//! }
//! ```

pub mod converter;
pub mod validator_extractor;

pub use converter::ProtobufParseResult;

use std::path::Path;

/// Error type for Protocol Buffers parsing operations.
#[derive(Debug)]
pub enum ProtobufError {
    /// IO error reading files.
    Io(std::io::Error),
    /// Protobuf parsing error.
    Parse(String),
}

impl std::fmt::Display for ProtobufError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtobufError::Io(e) => write!(f, "IO error: {}", e),
            ProtobufError::Parse(e) => write!(f, "Parse error: {}", e),
        }
    }
}

impl std::error::Error for ProtobufError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProtobufError::Io(e) => Some(e),
            ProtobufError::Parse(_) => None,
        }
    }
}

impl From<std::io::Error> for ProtobufError {
    fn from(e: std::io::Error) -> Self {
        ProtobufError::Io(e)
    }
}

/// Parse Protocol Buffers schema files and convert to evenframe types.
///
/// # Arguments
///
/// * `files` - Paths to the .proto files to parse
/// * `include_paths` - Additional directories to search for imports
///
/// # Returns
///
/// A `ProtobufParseResult` containing the converted structs and enums.
pub fn parse_protobuf_files(
    files: &[&Path],
    include_paths: &[&Path],
) -> Result<ProtobufParseResult, ProtobufError> {
    // Build include paths for protox
    let mut includes: Vec<_> = include_paths.iter().map(|p| p.to_path_buf()).collect();

    // Add the parent directories of the proto files as include paths
    for file in files {
        if let Some(parent) = file.parent()
            && !includes.contains(&parent.to_path_buf())
        {
            includes.push(parent.to_path_buf());
        }
    }

    // If no includes, use current directory
    if includes.is_empty() {
        includes.push(std::path::PathBuf::from("."));
    }

    // Create compiler with include paths
    let mut compiler = protox::Compiler::new(includes).map_err(|e| {
        ProtobufError::Parse(format!("Failed to create protox compiler: {}", e))
    })?;

    // Add files to compile
    for file in files {
        compiler.include_source_info(true);
        compiler
            .open_file(file)
            .map_err(|e| ProtobufError::Parse(format!("Failed to open {}: {}", file.display(), e)))?;
    }

    // Compile and get file descriptor set
    let fds = compiler.file_descriptor_set();

    // Convert to evenframe types
    Ok(converter::convert_descriptor_set(&fds))
}

/// Parse Protocol Buffers from source code.
///
/// # Arguments
///
/// * `name` - Virtual file name for the source
/// * `source` - The Protocol Buffers source code
///
/// # Returns
///
/// A `ProtobufParseResult` containing the converted structs and enums.
pub fn parse_protobuf_source(
    name: &str,
    source: &str,
) -> Result<ProtobufParseResult, ProtobufError> {
    use protox::file::{File, FileResolver};

    // Create a simple in-memory file resolver
    struct InMemoryResolver {
        name: String,
        source: String,
    }

    impl FileResolver for InMemoryResolver {
        fn open_file(&self, name: &str) -> Result<File, protox::Error> {
            if name == self.name {
                Ok(File::from_source(name, &self.source).map_err(|e| {
                    protox::Error::new(format!("Failed to parse source: {}", e))
                })?)
            } else {
                Err(protox::Error::new(format!("File not found: {}", name)))
            }
        }
    }

    let resolver = InMemoryResolver {
        name: name.to_string(),
        source: source.to_string(),
    };

    let mut compiler = protox::Compiler::with_file_resolver(resolver);
    compiler.include_source_info(true);
    compiler
        .open_file(name)
        .map_err(|e| ProtobufError::Parse(format!("Failed to parse {}: {}", name, e)))?;

    let fds = compiler.file_descriptor_set();

    Ok(converter::convert_descriptor_set(&fds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_proto() {
        let source = r#"
            syntax = "proto3";
            package example;

            message Person {
                string name = 1;
                int32 age = 2;
                bool active = 3;
            }
        "#;

        let result = parse_protobuf_source("test.proto", source).unwrap();

        assert_eq!(result.package, Some("example".to_string()));
        assert!(result.structs.contains_key("Person"));

        let person = &result.structs["Person"];
        assert_eq!(person.struct_name, "Person");
        assert_eq!(person.fields.len(), 3);
    }

    #[test]
    fn test_parse_proto_with_enum() {
        let source = r#"
            syntax = "proto3";

            enum Status {
                UNKNOWN = 0;
                ACTIVE = 1;
                INACTIVE = 2;
            }

            message User {
                string name = 1;
                Status status = 2;
            }
        "#;

        let result = parse_protobuf_source("test.proto", source).unwrap();

        assert!(result.enums.contains_key("Status"));
        let status = &result.enums["Status"];
        assert_eq!(status.variants.len(), 3);
        assert_eq!(status.variants[0].name, "UNKNOWN");
    }

    #[test]
    fn test_parse_proto_with_repeated() {
        let source = r#"
            syntax = "proto3";

            message Container {
                repeated string items = 1;
                repeated int32 numbers = 2;
            }
        "#;

        let result = parse_protobuf_source("test.proto", source).unwrap();

        let container = &result.structs["Container"];
        assert!(matches!(
            container.fields[0].field_type,
            crate::types::FieldType::Vec(_)
        ));
    }

    #[test]
    fn test_parse_proto_with_nested_message() {
        let source = r#"
            syntax = "proto3";

            message Outer {
                message Inner {
                    string value = 1;
                }
                Inner inner = 1;
            }
        "#;

        let result = parse_protobuf_source("test.proto", source).unwrap();

        assert!(result.structs.contains_key("Outer"));
        assert!(result.structs.contains_key("Inner"));
    }

    #[test]
    fn test_parse_proto_with_all_types() {
        let source = r#"
            syntax = "proto3";

            message AllTypes {
                double double_val = 1;
                float float_val = 2;
                int32 int32_val = 3;
                int64 int64_val = 4;
                uint32 uint32_val = 5;
                uint64 uint64_val = 6;
                sint32 sint32_val = 7;
                sint64 sint64_val = 8;
                fixed32 fixed32_val = 9;
                fixed64 fixed64_val = 10;
                sfixed32 sfixed32_val = 11;
                sfixed64 sfixed64_val = 12;
                bool bool_val = 13;
                string string_val = 14;
                bytes bytes_val = 15;
            }
        "#;

        let result = parse_protobuf_source("test.proto", source).unwrap();

        let all_types = &result.structs["AllTypes"];
        assert_eq!(all_types.fields.len(), 15);
    }
}
