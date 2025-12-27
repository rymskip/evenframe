//! FlatBuffers schema parser for evenframe.
//!
//! This module provides the ability to parse FlatBuffers schema files (.fbs)
//! and convert them to evenframe's internal type representations.
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use evenframe_core::typesync::flatbuffers_parser::parse_flatbuffers_file;
//!
//! let result = parse_flatbuffers_file(Path::new("schema.fbs"), &[]).unwrap();
//! println!("Found {} structs", result.structs.len());
//! println!("Found {} enums", result.enums.len());
//! ```
//!
//! # FlatBuffers Validation Attributes
//!
//! You can add validation metadata to FlatBuffers schemas:
//!
//! ```fbs
//! table User {
//!     email: string (validate: "email");
//!     age: int32 (validate: "min(0), max(150)");
//!     name: string (validate: "minLength(1), maxLength(100)");
//! }
//! ```

pub mod ast;
pub mod converter;
pub mod lexer;
pub mod parser;
pub mod validator_extractor;

pub use ast::{FbsType, FlatBuffersSchema, ScalarType};
pub use converter::FlatBuffersParseResult;
pub use lexer::LexError;
pub use parser::ParseError;

use std::path::Path;

/// Error type for FlatBuffers parsing operations.
#[derive(Debug)]
pub enum FlatBuffersError {
    /// IO error reading the file.
    Io(std::io::Error),
    /// Lexer error.
    Lex(LexError),
    /// Parser error.
    Parse(ParseError),
}

impl std::fmt::Display for FlatBuffersError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlatBuffersError::Io(e) => write!(f, "IO error: {}", e),
            FlatBuffersError::Lex(e) => write!(f, "Lexer error: {}", e),
            FlatBuffersError::Parse(e) => write!(f, "Parser error: {}", e),
        }
    }
}

impl std::error::Error for FlatBuffersError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FlatBuffersError::Io(e) => Some(e),
            FlatBuffersError::Lex(e) => Some(e),
            FlatBuffersError::Parse(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for FlatBuffersError {
    fn from(e: std::io::Error) -> Self {
        FlatBuffersError::Io(e)
    }
}

impl From<LexError> for FlatBuffersError {
    fn from(e: LexError) -> Self {
        FlatBuffersError::Lex(e)
    }
}

impl From<ParseError> for FlatBuffersError {
    fn from(e: ParseError) -> Self {
        FlatBuffersError::Parse(e)
    }
}

/// Parse a FlatBuffers schema file and convert to evenframe types.
///
/// # Arguments
///
/// * `path` - Path to the .fbs file
/// * `include_paths` - Additional directories to search for included files
///
/// # Returns
///
/// A `FlatBuffersParseResult` containing the converted structs and enums.
pub fn parse_flatbuffers_file(
    path: &Path,
    _include_paths: &[&Path],
) -> Result<FlatBuffersParseResult, FlatBuffersError> {
    let source = std::fs::read_to_string(path)?;
    parse_flatbuffers_source(&source)
}

/// Parse FlatBuffers schema source code and convert to evenframe types.
///
/// # Arguments
///
/// * `source` - The FlatBuffers schema source code
///
/// # Returns
///
/// A `FlatBuffersParseResult` containing the converted structs and enums.
pub fn parse_flatbuffers_source(source: &str) -> Result<FlatBuffersParseResult, FlatBuffersError> {
    let schema = parser::parse(source)?;
    let mut result = converter::convert_schema(&schema);

    // Extract validators from metadata
    for table in &schema.tables {
        if let Some(struct_config) = result.structs.get_mut(&table.name) {
            // Extract table-level validators
            struct_config.validators = validator_extractor::extract_table_validators(table);

            // Extract field-level validators
            for (field_def, struct_field) in
                table.fields.iter().zip(struct_config.fields.iter_mut())
            {
                struct_field.validators = validator_extractor::extract_field_validators(field_def);
            }
        }
    }

    Ok(result)
}

/// Parse multiple FlatBuffers schema files and merge results.
pub fn parse_flatbuffers_files(
    paths: &[&Path],
    include_paths: &[&Path],
) -> Result<FlatBuffersParseResult, FlatBuffersError> {
    let mut merged = FlatBuffersParseResult::new();

    for path in paths {
        let result = parse_flatbuffers_file(path, include_paths)?;

        // Merge structs
        merged.structs.extend(result.structs);

        // Merge enums
        merged.enums.extend(result.enums);

        // Take namespace from first file with a namespace
        if merged.namespace.is_none() {
            merged.namespace = result.namespace;
        }

        // Collect warnings
        merged.warnings.extend(result.warnings);
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_complete_schema() {
        let source = r#"
            namespace Game.Models;

            enum Status : byte {
                Active = 0,
                Inactive = 1,
                Pending = 2
            }

            table Player {
                id: uint64;
                name: string (validate: "minLength(1), maxLength(50)");
                email: string (validate: "email");
                level: int32 = 1;
                status: Status;
                scores: [int32];
            }

            table Team {
                name: string;
                members: [Player];
            }

            union GameEntity {
                Player,
                Team
            }

            root_type Player;
        "#;

        let result = parse_flatbuffers_source(source).unwrap();

        assert_eq!(result.namespace, Some("Game.Models".to_string()));
        assert!(result.structs.contains_key("Player"));
        assert!(result.structs.contains_key("Team"));
        assert!(result.enums.contains_key("Status"));
        assert!(result.enums.contains_key("GameEntity"));

        // Check Player fields
        let player = &result.structs["Player"];
        assert_eq!(player.fields.len(), 6);

        // Check name field has validators
        let name_field = player.fields.iter().find(|f| f.field_name == "name").unwrap();
        assert!(!name_field.validators.is_empty());

        // Check email field has validators
        let email_field = player
            .fields
            .iter()
            .find(|f| f.field_name == "email")
            .unwrap();
        assert!(!email_field.validators.is_empty());
    }

    #[test]
    fn test_parse_with_includes() {
        // Basic smoke test - includes are not fully implemented yet
        let source = r#"
            include "types.fbs";

            table User {
                name: string;
            }
        "#;

        let result = parse_flatbuffers_source(source).unwrap();
        assert!(result.structs.contains_key("User"));
    }
}
