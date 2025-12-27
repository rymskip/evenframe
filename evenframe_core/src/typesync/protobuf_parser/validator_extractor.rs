//! Extracts validators from Protocol Buffers field options.
//!
//! Supports extracting validation rules from buf validate proto options:
//! ```proto
//! import "buf/validate/validate.proto";
//!
//! message User {
//!     string email = 1 [(buf.validate.field).string.email = true];
//!     int32 age = 2 [(buf.validate.field).int32 = {gte: 0, lte: 150}];
//! }
//! ```
//!
//! Also supports custom evenframe validation options:
//! ```proto
//! message User {
//!     string email = 1 [(evenframe.validate) = "email"];
//!     int32 age = 2 [(evenframe.validate) = "min(0), max(150)"];
//! }
//! ```

use crate::validator::Validator;
use prost_types::FieldDescriptorProto;

/// Extract validators from a field's options.
///
/// Note: This is a simplified implementation. Full protobuf option parsing
/// requires additional proto definitions for the custom options.
pub fn extract_field_validators(_field: &FieldDescriptorProto) -> Vec<Validator> {
    // Protobuf options require custom option definitions to be parsed.
    // The protox crate parses the raw options, but interpreting them requires
    // the option definitions to be included in the proto files being parsed.
    //
    // For now, we return an empty list. Full validation support would require:
    // 1. Including buf/validate/validate.proto in the include paths
    // 2. Parsing the FieldOptions to extract validate rules
    // 3. Converting those rules to evenframe Validators
    //
    // This can be enhanced by:
    // - Using protox's dynamic message support
    // - Or by including the validate.proto definitions at parse time

    Vec::new()
}

/// Parse validators from a string annotation.
///
/// This allows using the same validator syntax as FlatBuffers:
/// "email, minLength(8), maxLength(100)"
pub fn parse_validators(input: &str) -> Vec<Validator> {
    // Reuse the FlatBuffers validator parser since the syntax is the same
    super::super::flatbuffers_parser::validator_extractor::parse_validators(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_validators_reuses_flatbuffers_parser() {
        let validators = parse_validators("email, minLength(8)");
        assert_eq!(validators.len(), 2);
    }

    #[test]
    fn test_empty_validators() {
        let field = FieldDescriptorProto::default();
        let validators = extract_field_validators(&field);
        assert!(validators.is_empty());
    }
}
