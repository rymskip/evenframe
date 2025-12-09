/// Edge case tests for validator macro bugs
/// Tests various combinations of validators with Option types, integer types,
/// nested structures, and multiple validators on the same field.

use evenframe::Evenframe;
use serde::Serialize;

// ==========================================
// EDGE CASE 1: Multiple validators on Option<String>
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct MultiValidatorOptionalString {
    pub id: String,

    /// Multiple string validators on Option<String>
    /// Should wrap ALL validators in a single Option check
    #[validators(
        StringValidator::MinLength(5),
        StringValidator::MaxLength(100),
        StringValidator::StartsWith("usr_")
    )]
    pub username: Option<String>,

    /// Email + max length on Option<String>
    #[validators(StringValidator::Email, StringValidator::MaxLength(255))]
    pub email: Option<String>,

    /// URL with max length on Option<String>
    #[validators(StringValidator::Url, StringValidator::MaxLength(2000))]
    pub website: Option<String>,
}

// ==========================================
// EDGE CASE 2: All integer types with NumberValidator
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct AllIntegerTypes {
    pub id: String,

    #[validators(NumberValidator::Positive)]
    pub positive_u8: u8,

    #[validators(NumberValidator::Positive)]
    pub positive_u16: u16,

    #[validators(NumberValidator::Positive)]
    pub positive_u32: u32,

    #[validators(NumberValidator::Positive)]
    pub positive_u64: u64,

    #[validators(NumberValidator::NonNegative)]
    pub non_negative_usize: usize,

    #[validators(NumberValidator::Positive)]
    pub positive_i8: i8,

    #[validators(NumberValidator::Positive)]
    pub positive_i16: i16,

    #[validators(NumberValidator::Positive)]
    pub positive_i32: i32,

    #[validators(NumberValidator::Positive)]
    pub positive_i64: i64,

    #[validators(NumberValidator::NonNegative)]
    pub non_negative_isize: isize,
}

// ==========================================
// EDGE CASE 3: Optional integer types with NumberValidator
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct OptionalIntegerValidation {
    pub id: String,

    /// Option<u32> with Positive validator
    #[validators(NumberValidator::Positive)]
    pub optional_positive_u32: Option<u32>,

    /// Option<i32> with NonNegative validator
    #[validators(NumberValidator::NonNegative)]
    pub optional_non_negative_i32: Option<i32>,

    /// Option<u64> with Positive validator
    #[validators(NumberValidator::Positive)]
    pub optional_positive_u64: Option<u64>,

    /// Option<i64> with Negative validator
    #[validators(NumberValidator::Negative)]
    pub optional_negative_i64: Option<i64>,
}

// ==========================================
// EDGE CASE 4: NumberValidator with bounds on integers
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct IntegerBounds {
    pub id: String,

    /// Between validator on u32
    #[validators(NumberValidator::Between(1.0, 100.0))]
    pub percentage: u32,

    /// GreaterThan on i32
    #[validators(NumberValidator::GreaterThan(0.0))]
    pub above_zero: i32,

    /// LessThan on u16
    #[validators(NumberValidator::LessThan(1000.0))]
    pub under_thousand: u16,

    /// Optional Between on u32
    #[validators(NumberValidator::Between(0.0, 255.0))]
    pub optional_byte_value: Option<u32>,

    /// GreaterThanOrEqualTo on Option<i32>
    #[validators(NumberValidator::GreaterThanOrEqualTo(-100.0))]
    pub optional_bounded: Option<i32>,
}

// ==========================================
// EDGE CASE 5: Complex string validators on Option<String>
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct ComplexStringValidation {
    pub id: String,

    /// UUID validation on Option<String>
    #[validators(StringValidator::Uuid)]
    pub optional_uuid: Option<String>,

    /// IP address on Option<String>
    #[validators(StringValidator::Ip)]
    pub optional_ip: Option<String>,

    /// IPv4 on Option<String>
    #[validators(StringValidator::IpV4)]
    pub optional_ipv4: Option<String>,

    /// Digits only on Option<String>
    #[validators(StringValidator::Digits)]
    pub optional_digits: Option<String>,

    /// Hex on Option<String>
    #[validators(StringValidator::Hex)]
    pub optional_hex: Option<String>,

    /// Alpha on Option<String>
    #[validators(StringValidator::Alpha)]
    pub optional_alpha: Option<String>,

    /// Alphanumeric on Option<String>
    #[validators(StringValidator::Alphanumeric)]
    pub optional_alphanumeric: Option<String>,

    /// EndsWith on Option<String>
    #[validators(StringValidator::EndsWith(".com"))]
    pub optional_domain: Option<String>,

    /// Includes on Option<String>
    #[validators(StringValidator::Includes("@"))]
    pub optional_with_at: Option<String>,
}

// ==========================================
// EDGE CASE 6: Case validators on Option<String>
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct CaseValidation {
    pub id: String,

    /// Lowercased on Option<String>
    #[validators(StringValidator::Lowercased)]
    pub optional_lowercase: Option<String>,

    /// Uppercased on Option<String>
    #[validators(StringValidator::Uppercased)]
    pub optional_uppercase: Option<String>,

    /// Capitalized on Option<String>
    #[validators(StringValidator::Capitalized)]
    pub optional_capitalized: Option<String>,

    /// Uncapitalized on Option<String>
    #[validators(StringValidator::Uncapitalized)]
    pub optional_uncapitalized: Option<String>,

    /// Trimmed on Option<String>
    #[validators(StringValidator::Trimmed)]
    pub optional_trimmed: Option<String>,
}

// ==========================================
// EDGE CASE 7: Nested struct with validators inside
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct InnerValidated {
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub name: String,

    #[validators(NumberValidator::Positive)]
    pub count: u32,

    #[validators(StringValidator::Email)]
    pub optional_email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct OuterWithNestedValidator {
    pub id: String,

    /// Inner struct is not wrapped in Option
    pub inner: InnerValidated,

    /// Inner struct wrapped in Option
    pub optional_inner: Option<InnerValidated>,

    /// Vec of inner structs
    pub inner_list: Vec<InnerValidated>,
}

// ==========================================
// EDGE CASE 8: Multiple NumberValidators on same field
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct MultipleNumberValidators {
    pub id: String,

    /// Multiple validators on same integer field
    #[validators(NumberValidator::Positive, NumberValidator::LessThan(100.0))]
    pub bounded_positive: u32,

    /// Multiple validators on optional integer
    #[validators(NumberValidator::NonNegative, NumberValidator::LessThanOrEqualTo(255.0))]
    pub optional_byte: Option<u32>,

    /// Positive + Between on f64
    #[validators(NumberValidator::Positive, NumberValidator::Between(0.01, 100.0))]
    pub percentage: f64,
}

// ==========================================
// EDGE CASE 9: Empty string handling
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct NonEmptyOptionString {
    pub id: String,

    /// NonEmpty on Option<String> - should only validate if Some
    #[validators(StringValidator::NonEmpty)]
    pub optional_non_empty: Option<String>,

    /// NonEmpty + MinLength on Option<String>
    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(3))]
    pub optional_min_three: Option<String>,
}

// ==========================================
// EDGE CASE 10: Mixed validators on f64
// ==========================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct FloatValidation {
    pub id: String,

    #[validators(NumberValidator::Positive)]
    pub positive_float: f64,

    #[validators(NumberValidator::NonNegative)]
    pub non_negative_float: f64,

    #[validators(NumberValidator::Between(0.0, 1.0))]
    pub normalized: f64,

    #[validators(NumberValidator::Positive)]
    pub optional_positive_float: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_validator_optional_string_with_some() {
        let item = MultiValidatorOptionalString {
            id: "test:1".to_string(),
            username: Some("usr_test123".to_string()),
            email: Some("test@example.com".to_string()),
            website: Some("https://example.com".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("usr_test123"));
    }

    #[test]
    fn test_multi_validator_optional_string_with_none() {
        let item = MultiValidatorOptionalString {
            id: "test:1".to_string(),
            username: None,
            email: None,
            website: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("null"));
    }

    #[test]
    fn test_all_integer_types() {
        let item = AllIntegerTypes {
            id: "test:1".to_string(),
            positive_u8: 1,
            positive_u16: 1,
            positive_u32: 1,
            positive_u64: 1,
            non_negative_usize: 0,
            positive_i8: 1,
            positive_i16: 1,
            positive_i32: 1,
            positive_i64: 1,
            non_negative_isize: 0,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"positive_u8\":1"));
    }

    #[test]
    fn test_optional_integer_validation_some() {
        let item = OptionalIntegerValidation {
            id: "test:1".to_string(),
            optional_positive_u32: Some(42),
            optional_non_negative_i32: Some(0),
            optional_positive_u64: Some(100),
            optional_negative_i64: Some(-5),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("42"));
    }

    #[test]
    fn test_optional_integer_validation_none() {
        let item = OptionalIntegerValidation {
            id: "test:1".to_string(),
            optional_positive_u32: None,
            optional_non_negative_i32: None,
            optional_positive_u64: None,
            optional_negative_i64: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("null"));
    }

    #[test]
    fn test_integer_bounds() {
        let item = IntegerBounds {
            id: "test:1".to_string(),
            percentage: 50,
            above_zero: 1,
            under_thousand: 999,
            optional_byte_value: Some(128),
            optional_bounded: Some(-50),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("50"));
    }

    #[test]
    fn test_complex_string_validation() {
        let item = ComplexStringValidation {
            id: "test:1".to_string(),
            optional_uuid: Some("550e8400-e29b-41d4-a716-446655440000".to_string()),
            optional_ip: Some("192.168.1.1".to_string()),
            optional_ipv4: Some("192.168.1.1".to_string()),
            optional_digits: Some("12345".to_string()),
            optional_hex: Some("deadbeef".to_string()),
            optional_alpha: Some("abcdef".to_string()),
            optional_alphanumeric: Some("abc123".to_string()),
            optional_domain: Some("example.com".to_string()),
            optional_with_at: Some("user@domain".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("550e8400"));
    }

    #[test]
    fn test_case_validation() {
        let item = CaseValidation {
            id: "test:1".to_string(),
            optional_lowercase: Some("lowercase".to_string()),
            optional_uppercase: Some("UPPERCASE".to_string()),
            optional_capitalized: Some("Capitalized".to_string()),
            optional_uncapitalized: Some("uncapitalized".to_string()),
            optional_trimmed: Some("no spaces".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("lowercase"));
    }

    #[test]
    fn test_nested_validated_structs() {
        let item = OuterWithNestedValidator {
            id: "test:1".to_string(),
            inner: InnerValidated {
                name: "test".to_string(),
                count: 5,
                optional_email: Some("test@example.com".to_string()),
            },
            optional_inner: None,
            inner_list: vec![],
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn test_multiple_number_validators() {
        let item = MultipleNumberValidators {
            id: "test:1".to_string(),
            bounded_positive: 50,
            optional_byte: Some(128),
            percentage: 50.0,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("50"));
    }

    #[test]
    fn test_non_empty_option_string() {
        let item = NonEmptyOptionString {
            id: "test:1".to_string(),
            optional_non_empty: Some("not empty".to_string()),
            optional_min_three: Some("abc".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("not empty"));
    }

    #[test]
    fn test_non_empty_option_string_none() {
        let item = NonEmptyOptionString {
            id: "test:1".to_string(),
            optional_non_empty: None,
            optional_min_three: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("null"));
    }

    #[test]
    fn test_float_validation() {
        let item = FloatValidation {
            id: "test:1".to_string(),
            positive_float: 1.5,
            non_negative_float: 0.0,
            normalized: 0.5,
            optional_positive_float: Some(3.14),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("1.5"));
    }
}
