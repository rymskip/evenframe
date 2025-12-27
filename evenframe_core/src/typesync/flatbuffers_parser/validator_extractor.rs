//! Extracts validators from FlatBuffers metadata attributes.
//!
//! FlatBuffers schemas can include validation metadata in the form:
//! ```fbs
//! table User {
//!     email: string (validate: "email");
//!     age: int32 (validate: "min(0), max(150)");
//!     password: string (validate: "minLength(8), maxLength(128)");
//! }
//! ```

use super::ast::{FieldDef, Metadata, TableDef};
use crate::validator::{ArrayValidator, NumberValidator, StringValidator, Validator};
use once_cell::sync::Lazy;
use ordered_float::OrderedFloat;
use regex::Regex;

static VALIDATOR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\w+)(?:\(([^)]*)\))?").expect("Invalid validator regex"));

/// Extract validators from a field's metadata.
pub fn extract_field_validators(field: &FieldDef) -> Vec<Validator> {
    extract_validators_from_metadata(&field.metadata)
}

/// Extract validators from a table's metadata (struct-level validators).
pub fn extract_table_validators(table: &TableDef) -> Vec<Validator> {
    extract_validators_from_metadata(&table.metadata)
}

fn extract_validators_from_metadata(metadata: &Metadata) -> Vec<Validator> {
    let Some(validate_str) = metadata.get("validate") else {
        return Vec::new();
    };

    parse_validators(validate_str)
}

/// Parse a comma-separated list of validators from a string.
///
/// Format: "validator1, validator2(arg), validator3(arg1, arg2)"
pub fn parse_validators(input: &str) -> Vec<Validator> {
    let mut validators = Vec::new();

    for capture in VALIDATOR_RE.captures_iter(input) {
        let name = capture.get(1).map(|m| m.as_str()).unwrap_or("");
        let args: Vec<&str> = capture
            .get(2)
            .map(|m| m.as_str().split(',').map(|s| s.trim()).collect())
            .unwrap_or_default();

        if let Some(validator) = parse_single_validator(name, &args) {
            validators.push(validator);
        }
    }

    validators
}

fn parse_single_validator(name: &str, args: &[&str]) -> Option<Validator> {
    // Normalize to lowercase for matching
    match name.to_lowercase().as_str() {
        // String validators
        "string" => Some(Validator::StringValidator(StringValidator::String)),
        "alpha" => Some(Validator::StringValidator(StringValidator::Alpha)),
        "alphanumeric" => Some(Validator::StringValidator(StringValidator::Alphanumeric)),
        "base64" => Some(Validator::StringValidator(StringValidator::Base64)),
        "base64url" => Some(Validator::StringValidator(StringValidator::Base64Url)),
        "capitalize" => Some(Validator::StringValidator(StringValidator::Capitalize)),
        "creditcard" | "credit_card" => {
            Some(Validator::StringValidator(StringValidator::CreditCard))
        }
        "date" => Some(Validator::StringValidator(StringValidator::Date)),
        "dateiso" | "date_iso" => Some(Validator::StringValidator(StringValidator::DateIso)),
        "digits" => Some(Validator::StringValidator(StringValidator::Digits)),
        "email" => Some(Validator::StringValidator(StringValidator::Email)),
        "hex" => Some(Validator::StringValidator(StringValidator::Hex)),
        "integer" => Some(Validator::StringValidator(StringValidator::Integer)),
        "ip" => Some(Validator::StringValidator(StringValidator::Ip)),
        "ipv4" | "ip_v4" => Some(Validator::StringValidator(StringValidator::IpV4)),
        "ipv6" | "ip_v6" => Some(Validator::StringValidator(StringValidator::IpV6)),
        "json" => Some(Validator::StringValidator(StringValidator::Json)),
        "lower" | "lowercase" => Some(Validator::StringValidator(StringValidator::Lower)),
        "upper" | "uppercase" => Some(Validator::StringValidator(StringValidator::Upper)),
        "nonempty" | "non_empty" => Some(Validator::StringValidator(StringValidator::NonEmpty)),
        "numeric" => Some(Validator::StringValidator(StringValidator::Numeric)),
        "regex" | "pattern" => {
            // StringValidator::Regex has no argument - it just validates that the field is a regex pattern
            Some(Validator::StringValidator(StringValidator::Regex))
        }
        "semver" => Some(Validator::StringValidator(StringValidator::Semver)),
        "trim" => Some(Validator::StringValidator(StringValidator::Trim)),
        "url" => Some(Validator::StringValidator(StringValidator::Url)),
        "uuid" => Some(Validator::StringValidator(StringValidator::Uuid)),
        "minlength" | "min_length" => args
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|len| Validator::StringValidator(StringValidator::MinLength(len))),
        "maxlength" | "max_length" => args
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|len| Validator::StringValidator(StringValidator::MaxLength(len))),
        "length" => args
            .first()
            .map(|len| Validator::StringValidator(StringValidator::Length(len.to_string()))),
        "startswith" | "starts_with" => args.first().map(|prefix| {
            Validator::StringValidator(StringValidator::StartsWith(
                prefix.trim_matches('"').to_string(),
            ))
        }),
        "endswith" | "ends_with" => args.first().map(|suffix| {
            Validator::StringValidator(StringValidator::EndsWith(
                suffix.trim_matches('"').to_string(),
            ))
        }),
        "includes" | "contains" => args.first().map(|substr| {
            Validator::StringValidator(StringValidator::Includes(
                substr.trim_matches('"').to_string(),
            ))
        }),

        // Number validators
        "min" | "gte" | "greaterthanorequalto" => args
            .first()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|val| {
                Validator::NumberValidator(NumberValidator::GreaterThanOrEqualTo(OrderedFloat(val)))
            }),
        "max" | "lte" | "lessthanorequalto" => args
            .first()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|val| {
                Validator::NumberValidator(NumberValidator::LessThanOrEqualTo(OrderedFloat(val)))
            }),
        "gt" | "greaterthan" => args
            .first()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|val| Validator::NumberValidator(NumberValidator::GreaterThan(OrderedFloat(val)))),
        "lt" | "lessthan" => args
            .first()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|val| Validator::NumberValidator(NumberValidator::LessThan(OrderedFloat(val)))),
        "positive" => Some(Validator::NumberValidator(NumberValidator::Positive)),
        "negative" => Some(Validator::NumberValidator(NumberValidator::Negative)),
        "nonpositive" | "non_positive" => {
            Some(Validator::NumberValidator(NumberValidator::NonPositive))
        }
        "nonnegative" | "non_negative" => {
            Some(Validator::NumberValidator(NumberValidator::NonNegative))
        }
        "int" => Some(Validator::NumberValidator(NumberValidator::Int)),
        "finite" => Some(Validator::NumberValidator(NumberValidator::Finite)),
        "nonnan" | "non_nan" => Some(Validator::NumberValidator(NumberValidator::NonNaN)),
        "multipleof" | "multiple_of" | "divisibleby" | "divisible_by" => args
            .first()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|val| Validator::NumberValidator(NumberValidator::MultipleOf(OrderedFloat(val)))),
        "between" => {
            if args.len() >= 2
                && let (Some(min), Some(max)) =
                    (args[0].parse::<f64>().ok(), args[1].parse::<f64>().ok())
            {
                Some(Validator::NumberValidator(NumberValidator::Between(
                    OrderedFloat(min),
                    OrderedFloat(max),
                )))
            } else {
                None
            }
        }

        // Array validators
        "minitems" | "min_items" | "minsize" | "min_size" => args
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|len| Validator::ArrayValidator(ArrayValidator::MinItems(len))),
        "maxitems" | "max_items" | "maxsize" | "max_size" => args
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|len| Validator::ArrayValidator(ArrayValidator::MaxItems(len))),
        "itemscount" | "items_count" | "size" => args
            .first()
            .and_then(|s| s.parse::<usize>().ok())
            .map(|len| Validator::ArrayValidator(ArrayValidator::ItemsCount(len))),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_email_validator() {
        let validators = parse_validators("email");
        assert_eq!(validators.len(), 1);
        assert!(matches!(
            validators[0],
            Validator::StringValidator(StringValidator::Email)
        ));
    }

    #[test]
    fn test_parse_multiple_validators() {
        let validators = parse_validators("email, minLength(8), maxLength(100)");
        assert_eq!(validators.len(), 3);
        assert!(matches!(
            validators[0],
            Validator::StringValidator(StringValidator::Email)
        ));
        assert!(matches!(
            validators[1],
            Validator::StringValidator(StringValidator::MinLength(8))
        ));
        assert!(matches!(
            validators[2],
            Validator::StringValidator(StringValidator::MaxLength(100))
        ));
    }

    #[test]
    fn test_parse_number_validators() {
        let validators = parse_validators("min(0), max(150), positive");
        assert_eq!(validators.len(), 3);
        assert!(matches!(
            validators[0],
            Validator::NumberValidator(NumberValidator::GreaterThanOrEqualTo(_))
        ));
        assert!(matches!(
            validators[1],
            Validator::NumberValidator(NumberValidator::LessThanOrEqualTo(_))
        ));
        assert!(matches!(
            validators[2],
            Validator::NumberValidator(NumberValidator::Positive)
        ));
    }

    #[test]
    fn test_parse_array_validators() {
        let validators = parse_validators("minItems(1), maxItems(10)");
        assert_eq!(validators.len(), 2);
        assert!(matches!(
            validators[0],
            Validator::ArrayValidator(ArrayValidator::MinItems(1))
        ));
        assert!(matches!(
            validators[1],
            Validator::ArrayValidator(ArrayValidator::MaxItems(10))
        ));
    }

    #[test]
    fn test_empty_input() {
        let validators = parse_validators("");
        assert!(validators.is_empty());
    }

    #[test]
    fn test_unknown_validator() {
        let validators = parse_validators("unknownValidator");
        assert!(validators.is_empty());
    }

    #[test]
    fn test_between_validator() {
        let validators = parse_validators("between(0, 100)");
        assert_eq!(validators.len(), 1);
        assert!(matches!(
            validators[0],
            Validator::NumberValidator(NumberValidator::Between(_, _))
        ));
    }
}
