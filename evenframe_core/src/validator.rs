use crate::schemasync::mockmake::format::Format;
use derive_more::From;
use ordered_float::OrderedFloat;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};
use try_from_expr::TryFromExpr;

#[derive(Debug, Clone, PartialEq, From, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum Validator {
    StringValidator(StringValidator),
    NumberValidator(NumberValidator),
    ArrayValidator(ArrayValidator),
    DateValidator(DateValidator),
    BigIntValidator(BigIntValidator),
    BigDecimalValidator(BigDecimalValidator),
    DurationValidator(DurationValidator),
}

/// Describes various string validation and transformation _requirements.
#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum StringValidator {
    /// A string
    String,

    /// Only letters
    Alpha,

    /// Only letters and digits 0-9
    Alphanumeric,

    /// Base64-encoded
    Base64,

    /// Base64url-encoded
    Base64Url,

    /// A morph from a string to capitalized
    Capitalize,

    /// Capitalized
    CapitalizePreformatted,

    /// A credit card number and a credit card number
    CreditCard,

    /// A string and a parsable date
    Date,

    /// An integer string representing a safe Unix timestamp
    DateEpoch,

    /// A morph from an integer string representing a safe Unix timestamp to a Date
    DateEpochParse,

    /// An ISO 8601 (YYYY-MM-DDTHH:mm:ss.sssZ) date
    DateIso,

    /// A morph from an ISO 8601 (YYYY-MM-DDTHH:mm:ss.sssZ) date to a Date
    DateIsoParse,

    /// A morph from a string and a parsable date to a Date
    DateParse,

    /// Only digits 0-9
    Digits,

    /// An email address
    Email,

    /// Hex characters only
    Hex,

    /// A well-formed integer string
    Integer,

    /// A morph from a well-formed integer string to an integer
    IntegerParse,

    /// An IP address
    Ip,

    /// An IPv4 address
    IpV4,

    /// An IPv6 address
    IpV6,

    /// A JSON string
    Json,

    /// Safe JSON string parser
    JsonParse,

    /// A morph from a string to only lowercase letters
    Lower,

    /// Only lowercase letters
    LowerPreformatted,

    /// A morph from a string to NFC-normalized unicode
    Normalize,

    /// A morph from a string to NFC-normalized unicode
    NormalizeNFC,

    /// NFC-normalized unicode
    NormalizeNFCPreformatted,

    /// A morph from a string to NFD-normalized unicode
    NormalizeNFD,

    /// NFD-normalized unicode
    NormalizeNFDPreformatted,

    /// A morph from a string to NFKC-normalized unicode
    NormalizeNFKC,

    /// NFKC-normalized unicode
    NormalizeNFKCPreformatted,

    /// A morph from a string to NFKD-normalized unicode
    NormalizeNFKD,

    /// NFKD-normalized unicode
    NormalizeNFKDPreformatted,

    /// A well-formed numeric string
    Numeric,

    /// A morph from a well-formed numeric string to a number
    NumericParse,

    /// A string and a regex pattern
    Regex,

    /// A semantic version (see https://semver.org/)
    Semver,

    /// A morph from a string to trimmed
    Trim,

    /// Trimmed
    TrimPreformatted,

    /// A morph from a string to only uppercase letters
    Upper,

    /// Only uppercase letters
    UpperPreformatted,

    /// A string and a URL string
    Url,

    /// A morph from a string and a URL string to a URL instance
    UrlParse,

    /// A UUID
    Uuid,

    /// A UUIDv1
    UuidV1,

    /// A UUIDv2
    UuidV2,

    /// A UUIDv3
    UuidV3,

    /// A UUIDv4
    UuidV4,

    /// A UUIDv5
    UuidV5,

    /// A UUIDv6
    UuidV6,

    /// A UUIDv7
    UuidV7,

    /// A UUIDv8
    UuidV8,

    Literal(String),

    StringEmbedded(String),

    RegexLiteral(Format),

    Length(String),

    /// Minimum length of a string
    MinLength(usize),

    /// Maximum length of a string  
    MaxLength(usize),

    /// Non-empty string (equivalent to MinLength(1))
    NonEmpty,

    /// String starts with a specific prefix
    StartsWith(String),

    /// String ends with a specific suffix
    EndsWith(String),

    /// String includes a specific substring
    Includes(String),

    /// String has no leading or trailing whitespace (validation only)
    Trimmed,

    /// String is entirely lowercase (validation only)
    Lowercased,

    /// String is entirely uppercase (validation only)
    Uppercased,

    /// String is capitalized (validation only)
    Capitalized,

    /// String is uncapitalized (validation only)
    Uncapitalized,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum NumberValidator {
    /// Number greater than a value
    GreaterThan(OrderedFloat<f64>),

    /// Number greater than or equal to a value
    GreaterThanOrEqualTo(OrderedFloat<f64>),

    /// Number less than a value
    LessThan(OrderedFloat<f64>),

    /// Number less than or equal to a value  
    LessThanOrEqualTo(OrderedFloat<f64>),

    /// Number between two values (inclusive)
    Between(OrderedFloat<f64>, OrderedFloat<f64>),

    /// Must be an integer
    Int,

    /// Must not be NaN
    NonNaN,

    /// Must be a finite number (not NaN, +Infinity, -Infinity)
    Finite,

    /// Must be positive (> 0)
    Positive,

    /// Must be non-negative (>= 0)
    NonNegative,

    /// Must be negative (< 0)
    Negative,

    /// Must be non-positive (<= 0)
    NonPositive,

    /// Must be evenly divisible by a value
    MultipleOf(OrderedFloat<f64>),

    /// 8-bit unsigned integer (0 to 255)
    Uint8,
}

/// Describes array validation filters
#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum ArrayValidator {
    /// Minimum number of items in the array
    MinItems(usize),

    /// Maximum number of items in the array
    MaxItems(usize),

    /// Exact number of items in the array
    ItemsCount(usize),
}

/// Describes date validation filters
#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum DateValidator {
    /// Must be a valid date (not Invalid Date)
    ValidDate,

    /// Date greater than a specific date
    GreaterThanDate(String),

    /// Date greater than or equal to a specific date
    GreaterThanOrEqualToDate(String),

    /// Date less than a specific date
    LessThanDate(String),

    /// Date less than or equal to a specific date
    LessThanOrEqualToDate(String),

    /// Date between two dates (inclusive)
    BetweenDate(String, String),
}

/// Describes BigInt validation filters
#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum BigIntValidator {
    /// BigInt greater than a value
    GreaterThanBigInt(String),

    /// BigInt greater than or equal to a value
    GreaterThanOrEqualToBigInt(String),

    /// BigInt less than a value
    LessThanBigInt(String),

    /// BigInt less than or equal to a value
    LessThanOrEqualToBigInt(String),

    /// BigInt between two values (inclusive)
    BetweenBigInt(String, String),

    /// Must be positive (> 0n)
    PositiveBigInt,

    /// Must be non-negative (>= 0n)
    NonNegativeBigInt,

    /// Must be negative (< 0n)
    NegativeBigInt,

    /// Must be non-positive (<= 0n)
    NonPositiveBigInt,
}

/// Describes BigDecimal validation filters
#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum BigDecimalValidator {
    /// BigDecimal greater than a value
    GreaterThanBigDecimal(String),

    /// BigDecimal greater than or equal to a value
    GreaterThanOrEqualToBigDecimal(String),

    /// BigDecimal less than a value
    LessThanBigDecimal(String),

    /// BigDecimal less than or equal to a value
    LessThanOrEqualToBigDecimal(String),

    /// BigDecimal between two values (inclusive)
    BetweenBigDecimal(String, String),

    /// Must be positive (> 0)
    PositiveBigDecimal,

    /// Must be non-negative (>= 0)
    NonNegativeBigDecimal,

    /// Must be negative (< 0)
    NegativeBigDecimal,

    /// Must be non-positive (<= 0)
    NonPositiveBigDecimal,
}

/// Describes Duration validation filters
#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, Serialize, Deserialize)]
pub enum DurationValidator {
    /// Duration greater than a value
    GreaterThanDuration(String),

    /// Duration greater than or equal to a value
    GreaterThanOrEqualToDuration(String),

    /// Duration less than a value
    LessThanDuration(String),

    /// Duration less than or equal to a value
    LessThanOrEqualToDuration(String),

    /// Duration between two values (inclusive)
    BetweenDuration(String, String),
}

impl Validator {
    /// Generates validation logic tokens for each validator variant
    /// Returns TokenStream that can be used in proc macros to generate validation code
    pub fn get_validation_logic_tokens(&self, value_ident: &str) -> TokenStream {
        debug!(validator_type = ?self, value_ident = %value_ident, "Generating validation logic tokens");
        trace!("Validator details: {:?}", self);
        let value = syn::Ident::new(value_ident, proc_macro2::Span::call_site());

        match self {
            // String Validators
            Validator::StringValidator(string_validator) => match string_validator {
                StringValidator::String => quote! {
                    // No validation needed - any string is valid
                },
                StringValidator::Alpha => quote! {
                    if !#value.chars().all(|c| c.is_alphabetic()) {
                        return Err(serde::de::Error::custom("value must contain only alphabetic characters"));
                    }
                },
                StringValidator::Alphanumeric => quote! {
                    if !#value.chars().all(|c| c.is_alphanumeric()) {
                        return Err(serde::de::Error::custom("value must contain only alphanumeric characters"));
                    }
                },
                StringValidator::Base64 => quote! {
                    use base64::{Engine as _, engine::general_purpose};
                    if general_purpose::STANDARD.decode(&#value).is_err() {
                        return Err(serde::de::Error::custom("invalid base64 encoding"));
                    }
                },
                StringValidator::Base64Url => quote! {
                    use base64::{Engine as _, engine::general_purpose};
                    if general_purpose::URL_SAFE.decode(&#value).is_err() {
                        return Err(serde::de::Error::custom("invalid base64url encoding"));
                    }
                },
                StringValidator::Capitalize => quote! {
                    // This is a transformation, not validation
                    let #value = {
                        let mut chars = &#value.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                        }
                    };
                },
                StringValidator::CapitalizePreformatted => quote! {
                    if #value.is_empty() {
                        return Err(serde::de::Error::custom("value cannot be empty"));
                    }
                    match #value.chars().next() {
                        Some(first_char) if !first_char.is_uppercase() => {
                            return Err(serde::de::Error::custom("value must be capitalized"));
                        }
                        None => {
                            return Err(serde::de::Error::custom("value cannot be empty"));
                        }
                        _ => {}
                    }
                },
                StringValidator::CreditCard => quote! {
                    // Luhn algorithm validation
                    let digits: Vec<u32> = &#value.chars()
                        .filter(|c| c.is_digit(10))
                        .map(|c| c.to_digit(10).unwrap())
                        .collect();

                    if digits.len() < 13 || digits.len() > 19 {
                        return Err(serde::de::Error::custom("invalid credit card number length"));
                    }

                    let mut sum = 0;
                    let parity = digits.len() % 2;
                    for (i, digit) in digits.iter().enumerate() {
                        let mut digit = *digit;
                        if i % 2 != parity {
                            digit *= 2;
                            if digit > 9 {
                                digit -= 9;
                            }
                        }
                        sum += digit;
                    }

                    if sum % 10 != 0 {
                        return Err(serde::de::Error::custom("invalid credit card number"));
                    }
                },
                StringValidator::Date => quote! {
                    // Basic date validation - actual implementation would depend on date library
                    if chrono::NaiveDate::parse_from_str(&#value, "%Y-%m-%d").is_err() {
                        return Err(serde::de::Error::custom("invalid date format"));
                    }
                },
                StringValidator::DateEpoch => quote! {
                    if #value.parse::<i64>().is_err() {
                        return Err(serde::de::Error::custom("invalid epoch timestamp"));
                    }
                },
                StringValidator::DateEpochParse => quote! {
                    // Transform epoch to date
                    let timestamp = &#value.parse::<i64>()
                        .map_err(|_| serde::de::Error::custom("invalid epoch timestamp"))?;
                    let #value = chrono::NaiveDateTime::from_timestamp_opt(timestamp, 0)
                        .ok_or_else(|| serde::de::Error::custom("invalid timestamp"))?;
                },
                StringValidator::DateIso => quote! {
                    if chrono::DateTime::<chrono::Utc>::parse_from_rfc3339(&#value).is_err() {
                        return Err(serde::de::Error::custom("invalid ISO 8601 date"));
                    }
                },
                StringValidator::DateIsoParse => quote! {
                    let #value = chrono::DateTime::<chrono::Utc>::parse_from_rfc3339(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid ISO 8601 date"))?;
                },
                StringValidator::DateParse => quote! {
                    // Generic date parsing - implementation would depend on format
                    let #value = chrono::NaiveDate::parse_from_str(&#value, "%Y-%m-%d")
                        .map_err(|_| serde::de::Error::custom("invalid date format"))?;
                },
                StringValidator::Digits => quote! {
                    if !#value.chars().all(|c| c.is_digit(10)) {
                        return Err(serde::de::Error::custom("value must contain only digits"));
                    }
                },
                StringValidator::Email => quote! {
                    // Basic email validation
                    let parts: Vec<&str> = #value.split('@').collect();
                    if parts.len() != 2 {
                        return Err(serde::de::Error::custom("invalid email format"));
                    }
                    if parts[0].is_empty() || parts[1].is_empty() {
                        return Err(serde::de::Error::custom("invalid email format"));
                    }
                    if !parts[1].contains('.') {
                        return Err(serde::de::Error::custom("invalid email domain"));
                    }
                },
                StringValidator::Hex => quote! {
                    if !#value.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(serde::de::Error::custom("value must contain only hexadecimal characters"));
                    }
                },
                StringValidator::Integer => quote! {
                    if #value.parse::<i64>().is_err() {
                        return Err(serde::de::Error::custom("value must be a valid integer"));
                    }
                },
                StringValidator::IntegerParse => quote! {
                    let #value = &#value.parse::<i64>()
                        .map_err(|_| serde::de::Error::custom("invalid integer"))?;
                },
                StringValidator::Ip => quote! {
                    if #value.parse::<std::net::IpAddr>().is_err() {
                        return Err(serde::de::Error::custom("invalid IP address"));
                    }
                },
                StringValidator::IpV4 => quote! {
                    if #value.parse::<std::net::Ipv4Addr>().is_err() {
                        return Err(serde::de::Error::custom("invalid IPv4 address"));
                    }
                },
                StringValidator::IpV6 => quote! {
                    if #value.parse::<std::net::Ipv6Addr>().is_err() {
                        return Err(serde::de::Error::custom("invalid IPv6 address"));
                    }
                },
                StringValidator::Json => quote! {
                    if serde_json::from_str::<serde_json::Value>(&#value).is_err() {
                        return Err(serde::de::Error::custom("invalid JSON"));
                    }
                },
                StringValidator::JsonParse => quote! {
                    let #value = serde_json::from_str::<serde_json::Value>(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid JSON"))?;
                },
                StringValidator::Lower => quote! {
                    let #value = &#value.to_lowercase();
                },
                StringValidator::LowerPreformatted => quote! {
                    if #value.chars().any(|c| c.is_alphabetic() && !c.is_lowercase()) {
                        return Err(serde::de::Error::custom("value must be lowercase"));
                    }
                },
                StringValidator::Normalize => quote! {
                    use unicode_normalization::UnicodeNormalization;
                    let #value = &#value.nfc().collect::<String>();
                },
                StringValidator::NormalizeNFC => quote! {
                    use unicode_normalization::UnicodeNormalization;
                    let #value = &#value.nfc().collect::<String>();
                },
                StringValidator::NormalizeNFCPreformatted => quote! {
                    use unicode_normalization::{UnicodeNormalization, is_nfc};
                    if !is_nfc(&#value) {
                        return Err(serde::de::Error::custom("value must be NFC normalized"));
                    }
                },
                StringValidator::NormalizeNFD => quote! {
                    use unicode_normalization::UnicodeNormalization;
                    let #value = &#value.nfd().collect::<String>();
                },
                StringValidator::NormalizeNFDPreformatted => quote! {
                    use unicode_normalization::{UnicodeNormalization, is_nfd};
                    if !is_nfd(&#value) {
                        return Err(serde::de::Error::custom("value must be NFD normalized"));
                    }
                },
                StringValidator::NormalizeNFKC => quote! {
                    use unicode_normalization::UnicodeNormalization;
                    let #value = &#value.nfkc().collect::<String>();
                },
                StringValidator::NormalizeNFKCPreformatted => quote! {
                    use unicode_normalization::{UnicodeNormalization, is_nfkc};
                    if !is_nfkc(&#value) {
                        return Err(serde::de::Error::custom("value must be NFKC normalized"));
                    }
                },
                StringValidator::NormalizeNFKD => quote! {
                    use unicode_normalization::UnicodeNormalization;
                    let #value = &#value.nfkd().collect::<String>();
                },
                StringValidator::NormalizeNFKDPreformatted => quote! {
                    use unicode_normalization::{UnicodeNormalization, is_nfkd};
                    if !is_nfkd(&#value) {
                        return Err(serde::de::Error::custom("value must be NFKD normalized"));
                    }
                },
                StringValidator::Numeric => quote! {
                    if #value.parse::<f64>().is_err() {
                        return Err(serde::de::Error::custom("value must be numeric"));
                    }
                },
                StringValidator::NumericParse => quote! {
                    let #value = &#value.parse::<f64>()
                        .map_err(|_| serde::de::Error::custom("invalid numeric value"))?;
                },
                StringValidator::Regex => quote! {
                    // Note: Regex pattern would need to be provided separately
                    // This is a placeholder
                },
                StringValidator::Semver => quote! {
                    if semver::Version::parse(&#value).is_err() {
                        return Err(serde::de::Error::custom("invalid semantic version"));
                    }
                },
                StringValidator::Trim => quote! {
                    let #value = &#value.trim().to_string();
                },
                StringValidator::TrimPreformatted => quote! {
                    if &#value != &#value.trim() {
                        return Err(serde::de::Error::custom("value must be trimmed"));
                    }
                },
                StringValidator::Upper => quote! {
                    let #value = &#value.to_uppercase();
                },
                StringValidator::UpperPreformatted => quote! {
                    if #value.chars().any(|c| c.is_alphabetic() && !c.is_uppercase()) {
                        return Err(serde::de::Error::custom("value must be uppercase"));
                    }
                },
                StringValidator::Url => quote! {
                    if ::evenframe::prelude::url::Url::parse(&#value).is_err() {
                        return Err(serde::de::Error::custom("invalid URL"));
                    }
                },
                StringValidator::UrlParse => quote! {
                    let #value = ::evenframe::prelude::url::Url::parse(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid URL"))?;
                },
                StringValidator::Uuid => quote! {
                    if ::evenframe::prelude::uuid::Uuid::parse_str(&#value).is_err() {
                        return Err(serde::de::Error::custom("invalid UUID"));
                    }
                },
                StringValidator::UuidV1 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::Mac) {
                        return Err(serde::de::Error::custom("UUID must be version 1"));
                    }
                },
                StringValidator::UuidV2 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::Dce) {
                        return Err(serde::de::Error::custom("UUID must be version 2"));
                    }
                },
                StringValidator::UuidV3 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::Md5) {
                        return Err(serde::de::Error::custom("UUID must be version 3"));
                    }
                },
                StringValidator::UuidV4 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::Random) {
                        return Err(serde::de::Error::custom("UUID must be version 4"));
                    }
                },
                StringValidator::UuidV5 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::Sha1) {
                        return Err(serde::de::Error::custom("UUID must be version 5"));
                    }
                },
                StringValidator::UuidV6 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::SortMac) {
                        return Err(serde::de::Error::custom("UUID must be version 6"));
                    }
                },
                StringValidator::UuidV7 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::SortRand) {
                        return Err(serde::de::Error::custom("UUID must be version 7"));
                    }
                },
                StringValidator::UuidV8 => quote! {
                    let uuid = ::evenframe::prelude::uuid::Uuid::parse_str(&#value)
                        .map_err(|_| serde::de::Error::custom("invalid UUID"))?;
                    if uuid.get_version() != Some(::evenframe::prelude::uuid::Version::Custom) {
                        return Err(serde::de::Error::custom("UUID must be version 8"));
                    }
                },
                StringValidator::Literal(literal) => {
                    quote! {
                        if #value != #literal {
                            return Err(serde::de::Error::custom(format!("value must be exactly '{}'", #literal)));
                        }
                    }
                }
                StringValidator::StringEmbedded(_embedded) => quote! {
                    // String embedded validation would be handled by external logic
                },
                StringValidator::RegexLiteral(format_variant) => {
                    // have to make it a string because Regex does not have ToTokens
                    let format_regex_string: String =
                        format_variant.to_owned().into_regex().to_string();
                    quote! {
                        {
                            static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
                                regex::Regex::new(#format_regex_string).expect("Invalid regex pattern")
                            });

                            if !RE.is_match(&#value) {
                                return Err(serde::de::Error::custom("value does not match pattern"));
                            }
                        }
                    }
                }
                StringValidator::Length(len_str) => {
                    quote! {
                        let expected_len: usize = #len_str.parse()
                            .map_err(|_| serde::de::Error::custom("invalid length specification"))?;
                        if #value.len() != expected_len {
                            return Err(serde::de::Error::custom(format!("value must be exactly {} characters", expected_len)));
                        }
                    }
                }
                StringValidator::MinLength(min_len) => {
                    quote! {
                        if #value.len() < #min_len {
                            return Err(serde::de::Error::custom(format!("value must be at least {} characters", #min_len)));
                        }
                    }
                }
                StringValidator::MaxLength(max_len) => {
                    quote! {
                        if #value.len() > #max_len {
                            return Err(serde::de::Error::custom(format!("value must be at most {} characters", #max_len)));
                        }
                    }
                }
                StringValidator::NonEmpty => quote! {
                    if #value.is_empty() {
                        return Err(serde::de::Error::custom("value cannot be empty"));
                    }
                },
                StringValidator::StartsWith(prefix) => {
                    quote! {
                        if !#value.starts_with(#prefix) {
                            return Err(serde::de::Error::custom(format!("value must start with '{}'", #prefix)));
                        }
                    }
                }
                StringValidator::EndsWith(suffix) => {
                    quote! {
                        if !#value.ends_with(#suffix) {
                            return Err(serde::de::Error::custom(format!("value must end with '{}'", #suffix)));
                        }
                    }
                }
                StringValidator::Includes(substring) => {
                    quote! {
                        if !#value.contains(#substring) {
                            return Err(serde::de::Error::custom(format!("value must contain '{}'", #substring)));
                        }
                    }
                }
                StringValidator::Trimmed => quote! {
                    if #value != #value.trim() {
                        return Err(serde::de::Error::custom("value must not have leading or trailing whitespace"));
                    }
                },
                StringValidator::Lowercased => quote! {
                    if #value.chars().any(|c| c.is_alphabetic() && !c.is_lowercase()) {
                        return Err(serde::de::Error::custom("value must be entirely lowercase"));
                    }
                },
                StringValidator::Uppercased => quote! {
                    if #value.chars().any(|c| c.is_alphabetic() && !c.is_uppercase()) {
                        return Err(serde::de::Error::custom("value must be entirely uppercase"));
                    }
                },
                StringValidator::Capitalized => quote! {
                    let mut chars = #value.chars();
                    match chars.next() {
                        Some(first) if !first.is_uppercase() => {
                            return Err(serde::de::Error::custom("value must be capitalized"));
                        }
                        None => return Err(serde::de::Error::custom("value cannot be empty")),
                        _ => {}
                    }
                    if chars.any(|c| c.is_alphabetic() && !c.is_lowercase()) {
                        return Err(serde::de::Error::custom("value must be capitalized (only first letter uppercase)"));
                    }
                },
                StringValidator::Uncapitalized => quote! {
                    let mut chars = #value.chars();
                    match chars.next() {
                        Some(first) if !first.is_lowercase() => {
                            return Err(serde::de::Error::custom("value must be uncapitalized"));
                        }
                        None => return Err(serde::de::Error::custom("value cannot be empty")),
                        _ => {}
                    }
                },
            },

            // Number Validators
            Validator::NumberValidator(number_validator) => match number_validator {
                NumberValidator::GreaterThan(min) => {
                    let min_val = min.0;
                    quote! {
                        if (#value as f64) <= #min_val {
                            return Err(serde::de::Error::custom(format!("value must be greater than {}", #min_val)));
                        }
                    }
                }
                NumberValidator::GreaterThanOrEqualTo(min) => {
                    let min_val = min.0;
                    quote! {
                        if (#value as f64) < #min_val {
                            return Err(serde::de::Error::custom(format!("value must be greater than or equal to {}", #min_val)));
                        }
                    }
                }
                NumberValidator::LessThan(max) => {
                    let max_val = max.0;
                    quote! {
                        if (#value as f64) >= #max_val {
                            return Err(serde::de::Error::custom(format!("value must be less than {}", #max_val)));
                        }
                    }
                }
                NumberValidator::LessThanOrEqualTo(max) => {
                    let max_val = max.0;
                    quote! {
                        if (#value as f64) > #max_val {
                            return Err(serde::de::Error::custom(format!("value must be less than or equal to {}", #max_val)));
                        }
                    }
                }
                NumberValidator::Between(min, max) => {
                    let min_val = min.0;
                    let max_val = max.0;
                    quote! {
                        if (#value as f64) < #min_val || (#value as f64) > #max_val {
                            return Err(serde::de::Error::custom(format!("value must be between {} and {} (inclusive)", #min_val, #max_val)));
                        }
                    }
                }
                NumberValidator::Int => quote! {
                    if #value.fract() != 0.0 {
                        return Err(serde::de::Error::custom("value must be an integer"));
                    }
                },
                NumberValidator::NonNaN => quote! {
                    if #value.is_nan() {
                        return Err(serde::de::Error::custom("value cannot be NaN"));
                    }
                },
                NumberValidator::Positive => quote! {
                    if (#value as f64) <= 0.0 {
                        return Err(serde::de::Error::custom("value must be positive"));
                    }
                },
                NumberValidator::Negative => quote! {
                    if (#value as f64) >= 0.0 {
                        return Err(serde::de::Error::custom("value must be negative"));
                    }
                },
                NumberValidator::NonPositive => quote! {
                    if (#value as f64) > 0.0 {
                        return Err(serde::de::Error::custom("value must be non-positive"));
                    }
                },
                NumberValidator::NonNegative => quote! {
                    if (#value as f64) < 0.0 {
                        return Err(serde::de::Error::custom("value must be non-negative"));
                    }
                },
                NumberValidator::Finite => quote! {
                    if !#value.is_finite() {
                        return Err(serde::de::Error::custom("value must be finite"));
                    }
                },
                NumberValidator::MultipleOf(divisor) => {
                    let divisor_val = divisor.0;
                    quote! {
                        if (#value % #divisor_val).abs() > f64::EPSILON {
                            return Err(serde::de::Error::custom(format!("value must be a multiple of {}", #divisor_val)));
                        }
                    }
                }
                NumberValidator::Uint8 => quote! {
                    if #value < 0.0 || #value > 255.0 || #value.fract() != 0.0 {
                        return Err(serde::de::Error::custom("value must be an 8-bit unsigned integer (0-255)"));
                    }
                },
            },

            // Array Validators
            Validator::ArrayValidator(array_validator) => match array_validator {
                ArrayValidator::MinItems(min_count) => {
                    quote! {
                        if #value.len() < #min_count {
                            return Err(serde::de::Error::custom(format!("array must have at least {} items", #min_count)));
                        }
                    }
                }
                ArrayValidator::MaxItems(max_count) => {
                    quote! {
                        if #value.len() > #max_count {
                            return Err(serde::de::Error::custom(format!("array must have at most {} items", #max_count)));
                        }
                    }
                }
                ArrayValidator::ItemsCount(exact_count) => {
                    quote! {
                        if #value.len() != #exact_count {
                            return Err(serde::de::Error::custom(format!("array must have exactly {} items", #exact_count)));
                        }
                    }
                }
            },

            // Date Validators
            Validator::DateValidator(date_validator) => match date_validator {
                DateValidator::ValidDate => quote! {
                    // Assumes value is already parsed as a date type
                    // Validation would depend on the date library being used
                },
                DateValidator::GreaterThanDate(date_str) => {
                    quote! {
                        let compare_date = chrono::NaiveDate::parse_from_str(#date_str, "%Y-%m-%d")
                            .map_err(|_| serde::de::Error::custom("invalid comparison date"))?;
                        if #value <= compare_date {
                            return Err(serde::de::Error::custom(format!("date must be after {}", #date_str)));
                        }
                    }
                }
                DateValidator::GreaterThanOrEqualToDate(date_str) => {
                    quote! {
                        let compare_date = chrono::NaiveDate::parse_from_str(#date_str, "%Y-%m-%d")
                            .map_err(|_| serde::de::Error::custom("invalid comparison date"))?;
                        if #value < compare_date {
                            return Err(serde::de::Error::custom(format!("date must be on or after {}", #date_str)));
                        }
                    }
                }
                DateValidator::LessThanDate(date_str) => {
                    quote! {
                        let compare_date = chrono::NaiveDate::parse_from_str(#date_str, "%Y-%m-%d")
                            .map_err(|_| serde::de::Error::custom("invalid comparison date"))?;
                        if #value >= compare_date {
                            return Err(serde::de::Error::custom(format!("date must be before {}", #date_str)));
                        }
                    }
                }
                DateValidator::LessThanOrEqualToDate(date_str) => {
                    quote! {
                        let compare_date = chrono::NaiveDate::parse_from_str(#date_str, "%Y-%m-%d")
                            .map_err(|_| serde::de::Error::custom("invalid comparison date"))?;
                        if #value > compare_date {
                            return Err(serde::de::Error::custom(format!("date must be on or before {}", #date_str)));
                        }
                    }
                }
                DateValidator::BetweenDate(start_str, end_str) => {
                    quote! {
                        let start_date = chrono::NaiveDate::parse_from_str(#start_str, "%Y-%m-%d")
                            .map_err(|_| serde::de::Error::custom("invalid start date"))?;
                        let end_date = chrono::NaiveDate::parse_from_str(#end_str, "%Y-%m-%d")
                            .map_err(|_| serde::de::Error::custom("invalid end date"))?;
                        if #value < start_date || #value > end_date {
                            return Err(serde::de::Error::custom(format!("date must be between {} and {}", #start_str, #end_str)));
                        }
                    }
                }
            },

            // BigInt Validators
            Validator::BigIntValidator(bigint_validator) => match bigint_validator {
                BigIntValidator::GreaterThanBigInt(value_str) => {
                    quote! {
                        use num_bigint::BigInt;
                        use std::str::FromStr;
                        let compare_val = BigInt::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value <= compare_val {
                            return Err(serde::de::Error::custom(format!("value must be greater than {}", #value_str)));
                        }
                    }
                }
                BigIntValidator::GreaterThanOrEqualToBigInt(value_str) => {
                    quote! {
                        use num_bigint::BigInt;
                        use std::str::FromStr;
                        let compare_val = BigInt::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value < compare_val {
                            return Err(serde::de::Error::custom(format!("value must be greater than or equal to {}", #value_str)));
                        }
                    }
                }
                BigIntValidator::LessThanBigInt(value_str) => {
                    quote! {
                        use num_bigint::BigInt;
                        use std::str::FromStr;
                        let compare_val = BigInt::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value >= compare_val {
                            return Err(serde::de::Error::custom(format!("value must be less than {}", #value_str)));
                        }
                    }
                }
                BigIntValidator::LessThanOrEqualToBigInt(value_str) => {
                    quote! {
                        use num_bigint::BigInt;
                        use std::str::FromStr;
                        let compare_val = BigInt::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value > compare_val {
                            return Err(serde::de::Error::custom(format!("value must be less than or equal to {}", #value_str)));
                        }
                    }
                }
                BigIntValidator::BetweenBigInt(start_str, end_str) => {
                    quote! {
                        use num_bigint::BigInt;
                        use std::str::FromStr;
                        let start_val = BigInt::from_str(#start_str)
                            .map_err(|_| serde::de::Error::custom("invalid start value"))?;
                        let end_val = BigInt::from_str(#end_str)
                            .map_err(|_| serde::de::Error::custom("invalid end value"))?;
                        if #value < start_val || #value > end_val {
                            return Err(serde::de::Error::custom(format!("value must be between {} and {}", #start_str, #end_str)));
                        }
                    }
                }
                BigIntValidator::PositiveBigInt => {
                    quote! {
                        use num_bigint::BigInt;
                        use num_traits::Zero;
                        if #value <= BigInt::zero() {
                            return Err(serde::de::Error::custom("value must be positive"));
                        }
                    }
                }
                BigIntValidator::NonNegativeBigInt => {
                    quote! {
                        use num_bigint::BigInt;
                        use num_traits::Zero;
                        if #value < BigInt::zero() {
                            return Err(serde::de::Error::custom("value must be non-negative"));
                        }
                    }
                }
                BigIntValidator::NegativeBigInt => {
                    quote! {
                        use num_bigint::BigInt;
                        use num_traits::Zero;
                        if #value >= BigInt::zero() {
                            return Err(serde::de::Error::custom("value must be negative"));
                        }
                    }
                }
                BigIntValidator::NonPositiveBigInt => {
                    quote! {
                        use num_bigint::BigInt;
                        use num_traits::Zero;
                        if #value > BigInt::zero() {
                            return Err(serde::de::Error::custom("value must be non-positive"));
                        }
                    }
                }
            },

            // BigDecimal Validators
            Validator::BigDecimalValidator(bigdecimal_validator) => match bigdecimal_validator {
                BigDecimalValidator::GreaterThanBigDecimal(value_str) => {
                    quote! {
                        use bigdecimal::BigDecimal;
                        use std::str::FromStr;
                        let compare_val = BigDecimal::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value <= compare_val {
                            return Err(serde::de::Error::custom(format!("value must be greater than {}", #value_str)));
                        }
                    }
                }
                BigDecimalValidator::GreaterThanOrEqualToBigDecimal(value_str) => {
                    quote! {
                        use bigdecimal::BigDecimal;
                        use std::str::FromStr;
                        let compare_val = BigDecimal::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value < compare_val {
                            return Err(serde::de::Error::custom(format!("value must be greater than or equal to {}", #value_str)));
                        }
                    }
                }
                BigDecimalValidator::LessThanBigDecimal(value_str) => {
                    quote! {
                        use bigdecimal::BigDecimal;
                        use std::str::FromStr;
                        let compare_val = BigDecimal::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value >= compare_val {
                            return Err(serde::de::Error::custom(format!("value must be less than {}", #value_str)));
                        }
                    }
                }
                BigDecimalValidator::LessThanOrEqualToBigDecimal(value_str) => {
                    quote! {
                        use bigdecimal::BigDecimal;
                        use std::str::FromStr;
                        let compare_val = BigDecimal::from_str(#value_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison value"))?;
                        if #value > compare_val {
                            return Err(serde::de::Error::custom(format!("value must be less than or equal to {}", #value_str)));
                        }
                    }
                }
                BigDecimalValidator::BetweenBigDecimal(start_str, end_str) => {
                    quote! {
                        use bigdecimal::BigDecimal;
                        use std::str::FromStr;
                        let start_val = BigDecimal::from_str(#start_str)
                            .map_err(|_| serde::de::Error::custom("invalid start value"))?;
                        let end_val = BigDecimal::from_str(#end_str)
                            .map_err(|_| serde::de::Error::custom("invalid end value"))?;
                        if #value < start_val || #value > end_val {
                            return Err(serde::de::Error::custom(format!("value must be between {} and {}", #start_str, #end_str)));
                        }
                    }
                }
                BigDecimalValidator::PositiveBigDecimal => {
                    quote! {
                        use bigdecimal::{BigDecimal, Zero};
                        if #value <= BigDecimal::zero() {
                            return Err(serde::de::Error::custom("value must be positive"));
                        }
                    }
                }
                BigDecimalValidator::NonNegativeBigDecimal => {
                    quote! {
                        use bigdecimal::{BigDecimal, Zero};
                        if #value < BigDecimal::zero() {
                            return Err(serde::de::Error::custom("value must be non-negative"));
                        }
                    }
                }
                BigDecimalValidator::NegativeBigDecimal => {
                    quote! {
                        use bigdecimal::{BigDecimal, Zero};
                        if #value >= BigDecimal::zero() {
                            return Err(serde::de::Error::custom("value must be negative"));
                        }
                    }
                }
                BigDecimalValidator::NonPositiveBigDecimal => {
                    quote! {
                        use bigdecimal::{BigDecimal, Zero};
                        if #value > BigDecimal::zero() {
                            return Err(serde::de::Error::custom("value must be non-positive"));
                        }
                    }
                }
            },

            // Duration Validators
            Validator::DurationValidator(duration_validator) => match duration_validator {
                DurationValidator::GreaterThanDuration(duration_str) => {
                    quote! {
                        let compare_duration = parse_duration(#duration_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison duration"))?;
                        if #value <= compare_duration {
                            return Err(serde::de::Error::custom(format!("duration must be greater than {}", #duration_str)));
                        }
                    }
                }
                DurationValidator::GreaterThanOrEqualToDuration(duration_str) => {
                    quote! {
                        let compare_duration = parse_duration(#duration_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison duration"))?;
                        if #value < compare_duration {
                            return Err(serde::de::Error::custom(format!("duration must be greater than or equal to {}", #duration_str)));
                        }
                    }
                }
                DurationValidator::LessThanDuration(duration_str) => {
                    quote! {
                        let compare_duration = parse_duration(#duration_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison duration"))?;
                        if #value >= compare_duration {
                            return Err(serde::de::Error::custom(format!("duration must be less than {}", #duration_str)));
                        }
                    }
                }
                DurationValidator::LessThanOrEqualToDuration(duration_str) => {
                    quote! {
                        let compare_duration = parse_duration(#duration_str)
                            .map_err(|_| serde::de::Error::custom("invalid comparison duration"))?;
                        if #value > compare_duration {
                            return Err(serde::de::Error::custom(format!("duration must be less than or equal to {}", #duration_str)));
                        }
                    }
                }
                DurationValidator::BetweenDuration(start_str, end_str) => {
                    quote! {
                        let start_duration = parse_duration(#start_str)
                            .map_err(|_| serde::de::Error::custom("invalid start duration"))?;
                        let end_duration = parse_duration(#end_str)
                            .map_err(|_| serde::de::Error::custom("invalid end duration"))?;
                        if #value < start_duration || #value > end_duration {
                            return Err(serde::de::Error::custom(format!("duration must be between {} and {}", #start_str, #end_str)));
                        }
                    }
                }
            },
        }
    }
}

/// Runtime value passed to [`Validator::matches`].
///
/// Mockmake builds one of these for each candidate it generates and asks every
/// validator on the field whether it would accept the value. Validators that
/// don't apply to the supplied variant (e.g. a `NumberValidator` against a
/// `Str`) return `true` — they have no opinion on a value outside their
/// domain.
#[derive(Debug, Clone, Copy)]
pub enum MockValue<'a> {
    Str(&'a str),
    Num(f64),
    /// Lexical bigint without any suffix — e.g. `"1000000000000"`.
    BigInt(&'a str),
    /// Lexical bigdecimal — e.g. `"3.14159"`.
    BigDecimal(&'a str),
    /// Duration as nanoseconds. Mock values for SurrealDB durations are
    /// ultimately emitted as `duration::from_nanos(...)`, so this is the
    /// canonical unit for cross-validator comparison.
    DurationNanos(i128),
    Date(chrono::NaiveDate),
    ArrayLen(usize),
}

/// Parse a SurrealDB-style duration string (`"1h"`, `"30m500ms"`, `"1d"`,
/// `"1y"`) into nanoseconds. Returns `None` if the string is malformed.
///
/// Supports the same suffix set the SurrealQL parser does: `ns`, `us`, `µs`,
/// `ms`, `s`, `m`, `h`, `d`, `w`, `y`. Mixed suffixes are summed
/// (`"1h30m"` → 5_400 * 1e9).
pub fn parse_duration_to_nanos(s: &str) -> Option<i128> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // ns
    const NS: i128 = 1;
    const US: i128 = 1_000;
    const MS: i128 = 1_000_000;
    const SEC: i128 = 1_000_000_000;
    const MIN: i128 = 60 * SEC;
    const HOUR: i128 = 60 * MIN;
    const DAY: i128 = 24 * HOUR;
    const WEEK: i128 = 7 * DAY;
    // SurrealDB year = 365 days
    const YEAR: i128 = 365 * DAY;

    let mut total: i128 = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // parse digits
        let num_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == num_start {
            return None;
        }
        let num: i128 = s[num_start..i].parse().ok()?;
        // parse suffix
        let suf_start = i;
        // µs has a multibyte char; handle that first
        if s[suf_start..].starts_with("µs") {
            total = total.checked_add(num.checked_mul(US)?)?;
            i = suf_start + "µs".len();
            continue;
        }
        while i < bytes.len() && !bytes[i].is_ascii_digit() {
            i += 1;
        }
        let suffix = &s[suf_start..i];
        let mult = match suffix {
            "ns" => NS,
            "us" => US,
            "ms" => MS,
            "s" => SEC,
            "m" => MIN,
            "h" => HOUR,
            "d" => DAY,
            "w" => WEEK,
            "y" => YEAR,
            _ => return None,
        };
        total = total.checked_add(num.checked_mul(mult)?)?;
    }
    Some(total)
}

impl Validator {
    /// Runtime predicate: does `value` satisfy this validator?
    ///
    /// Used by mockmake to verify candidate values before emitting them.
    /// Validators that don't apply to the variant of `MockValue` return
    /// `true` (they have no opinion). Variants that have no clean Rust check
    /// (transformation morphs whose effect is supposed to be applied
    /// post-hoc, or placeholders like `StringValidator::Regex` with no
    /// pattern argument) also return `true`.
    pub fn matches(&self, value: &MockValue) -> bool {
        match self {
            Validator::StringValidator(sv) => match value {
                MockValue::Str(s) => match_string_validator(sv, s),
                _ => true,
            },
            Validator::NumberValidator(nv) => match value {
                MockValue::Num(n) => match_number_validator(nv, *n),
                _ => true,
            },
            Validator::ArrayValidator(av) => match value {
                MockValue::ArrayLen(len) => match av {
                    ArrayValidator::MinItems(min) => *len >= *min,
                    ArrayValidator::MaxItems(max) => *len <= *max,
                    ArrayValidator::ItemsCount(exact) => *len == *exact,
                },
                _ => true,
            },
            Validator::DateValidator(dv) => match value {
                MockValue::Date(d) => match_date_validator(dv, d),
                _ => true,
            },
            Validator::BigIntValidator(bv) => match value {
                MockValue::BigInt(s) => match_bigint_validator(bv, s),
                _ => true,
            },
            Validator::BigDecimalValidator(bv) => match value {
                MockValue::BigDecimal(s) => match_bigdecimal_validator(bv, s),
                _ => true,
            },
            Validator::DurationValidator(dv) => match value {
                MockValue::DurationNanos(n) => match_duration_validator(dv, *n),
                _ => true,
            },
        }
    }
}

fn match_string_validator(sv: &StringValidator, s: &str) -> bool {
    match sv {
        StringValidator::String => true,
        StringValidator::Alpha => s.chars().all(|c| c.is_alphabetic()),
        StringValidator::Alphanumeric => s.chars().all(|c| c.is_alphanumeric()),
        StringValidator::Hex => s.chars().all(|c| c.is_ascii_hexdigit()),
        StringValidator::Digits => !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
        StringValidator::Numeric => s.parse::<f64>().is_ok(),
        StringValidator::NumericParse => s.parse::<f64>().is_ok(),
        StringValidator::Integer => s.parse::<i64>().is_ok(),
        StringValidator::IntegerParse => s.parse::<i64>().is_ok(),
        StringValidator::Email => {
            let parts: Vec<&str> = s.split('@').collect();
            parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() && parts[1].contains('.')
        }
        StringValidator::Ip => s.parse::<std::net::IpAddr>().is_ok(),
        StringValidator::IpV4 => s.parse::<std::net::Ipv4Addr>().is_ok(),
        StringValidator::IpV6 => s.parse::<std::net::Ipv6Addr>().is_ok(),
        StringValidator::Uuid => uuid::Uuid::parse_str(s).is_ok(),
        StringValidator::UuidV1 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::Mac)
            .unwrap_or(false),
        StringValidator::UuidV2 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::Dce)
            .unwrap_or(false),
        StringValidator::UuidV3 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::Md5)
            .unwrap_or(false),
        StringValidator::UuidV4 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::Random)
            .unwrap_or(false),
        StringValidator::UuidV5 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::Sha1)
            .unwrap_or(false),
        StringValidator::UuidV6 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::SortMac)
            .unwrap_or(false),
        StringValidator::UuidV7 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::SortRand)
            .unwrap_or(false),
        StringValidator::UuidV8 => uuid::Uuid::parse_str(s)
            .ok()
            .and_then(|u| u.get_version())
            .map(|v| v == uuid::Version::Custom)
            .unwrap_or(false),
        StringValidator::Json => serde_json::from_str::<serde_json::Value>(s).is_ok(),
        StringValidator::JsonParse => serde_json::from_str::<serde_json::Value>(s).is_ok(),
        StringValidator::Date => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok(),
        StringValidator::DateEpoch => s.parse::<i64>().is_ok(),
        StringValidator::DateEpochParse => s.parse::<i64>().is_ok(),
        StringValidator::DateIso => chrono::DateTime::parse_from_rfc3339(s).is_ok(),
        StringValidator::DateIsoParse => chrono::DateTime::parse_from_rfc3339(s).is_ok(),
        StringValidator::DateParse => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok(),
        StringValidator::CreditCard => {
            let digits: Vec<u32> = s
                .chars()
                .filter(|c| c.is_ascii_digit())
                .filter_map(|c| c.to_digit(10))
                .collect();
            if !(13..=19).contains(&digits.len()) {
                return false;
            }
            let parity = digits.len() % 2;
            let sum: u32 = digits
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let mut d = *d;
                    if i % 2 != parity {
                        d *= 2;
                        if d > 9 {
                            d -= 9;
                        }
                    }
                    d
                })
                .sum();
            sum % 10 == 0
        }
        StringValidator::Literal(literal) => s == literal,
        StringValidator::Length(len_str) => match len_str.parse::<usize>() {
            Ok(expected) => s.chars().count() == expected,
            Err(_) => true,
        },
        StringValidator::MinLength(min) => s.chars().count() >= *min,
        StringValidator::MaxLength(max) => s.chars().count() <= *max,
        StringValidator::NonEmpty => !s.is_empty(),
        StringValidator::StartsWith(prefix) => s.starts_with(prefix.as_str()),
        StringValidator::EndsWith(suffix) => s.ends_with(suffix.as_str()),
        StringValidator::Includes(needle) => s.contains(needle.as_str()),
        StringValidator::Trimmed | StringValidator::TrimPreformatted => s == s.trim(),
        StringValidator::Trim => s == s.trim(),
        StringValidator::Lowercased | StringValidator::LowerPreformatted | StringValidator::Lower => {
            !s.chars().any(|c| c.is_alphabetic() && !c.is_lowercase())
        }
        StringValidator::Uppercased | StringValidator::UpperPreformatted | StringValidator::Upper => {
            !s.chars().any(|c| c.is_alphabetic() && !c.is_uppercase())
        }
        StringValidator::Capitalized | StringValidator::CapitalizePreformatted | StringValidator::Capitalize => {
            let mut chars = s.chars();
            let Some(first) = chars.next() else {
                return false;
            };
            if !first.is_uppercase() {
                return false;
            }
            !chars.any(|c| c.is_alphabetic() && !c.is_lowercase())
        }
        StringValidator::Uncapitalized => {
            let Some(first) = s.chars().next() else {
                return false;
            };
            first.is_lowercase()
        }
        StringValidator::RegexLiteral(format_variant) => {
            let re = format_variant.clone().into_regex();
            re.is_match(s)
        }
        // Variants without an attached pattern or with no clean Rust check —
        // mockmake's constraint generator will produce values that satisfy
        // them, and the retry loop has no way to second-guess them, so
        // accept by default.
        StringValidator::Regex
        | StringValidator::StringEmbedded(_)
        | StringValidator::Base64
        | StringValidator::Base64Url
        | StringValidator::Url
        | StringValidator::UrlParse
        | StringValidator::Semver
        | StringValidator::Normalize
        | StringValidator::NormalizeNFC
        | StringValidator::NormalizeNFCPreformatted
        | StringValidator::NormalizeNFD
        | StringValidator::NormalizeNFDPreformatted
        | StringValidator::NormalizeNFKC
        | StringValidator::NormalizeNFKCPreformatted
        | StringValidator::NormalizeNFKD
        | StringValidator::NormalizeNFKDPreformatted => true,
    }
}

fn match_number_validator(nv: &NumberValidator, n: f64) -> bool {
    match nv {
        NumberValidator::GreaterThan(min) => n > min.0,
        NumberValidator::GreaterThanOrEqualTo(min) => n >= min.0,
        NumberValidator::LessThan(max) => n < max.0,
        NumberValidator::LessThanOrEqualTo(max) => n <= max.0,
        NumberValidator::Between(min, max) => n >= min.0 && n <= max.0,
        NumberValidator::Int => n.is_finite() && n.fract() == 0.0,
        NumberValidator::NonNaN => !n.is_nan(),
        NumberValidator::Finite => n.is_finite(),
        NumberValidator::Positive => n > 0.0,
        NumberValidator::NonNegative => n >= 0.0,
        NumberValidator::Negative => n < 0.0,
        NumberValidator::NonPositive => n <= 0.0,
        NumberValidator::MultipleOf(divisor) => {
            let d = divisor.0;
            d != 0.0 && (n % d).abs() < f64::EPSILON
        }
        NumberValidator::Uint8 => n >= 0.0 && n <= 255.0 && n.fract() == 0.0,
    }
}

fn match_date_validator(dv: &DateValidator, d: &chrono::NaiveDate) -> bool {
    match dv {
        DateValidator::ValidDate => true,
        DateValidator::GreaterThanDate(s) => match chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            Ok(other) => *d > other,
            Err(_) => true,
        },
        DateValidator::GreaterThanOrEqualToDate(s) => {
            match chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                Ok(other) => *d >= other,
                Err(_) => true,
            }
        }
        DateValidator::LessThanDate(s) => match chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            Ok(other) => *d < other,
            Err(_) => true,
        },
        DateValidator::LessThanOrEqualToDate(s) => {
            match chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                Ok(other) => *d <= other,
                Err(_) => true,
            }
        }
        DateValidator::BetweenDate(start, end) => {
            let lo = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d");
            let hi = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d");
            match (lo, hi) {
                (Ok(lo), Ok(hi)) => *d >= lo && *d <= hi,
                _ => true,
            }
        }
    }
}

fn match_bigint_validator(bv: &BigIntValidator, s: &str) -> bool {
    let n: i128 = match s.parse() {
        Ok(n) => n,
        Err(_) => return true,
    };
    let parse = |x: &str| x.parse::<i128>().ok();
    match bv {
        BigIntValidator::GreaterThanBigInt(b) => parse(b).map(|b| n > b).unwrap_or(true),
        BigIntValidator::GreaterThanOrEqualToBigInt(b) => parse(b).map(|b| n >= b).unwrap_or(true),
        BigIntValidator::LessThanBigInt(b) => parse(b).map(|b| n < b).unwrap_or(true),
        BigIntValidator::LessThanOrEqualToBigInt(b) => parse(b).map(|b| n <= b).unwrap_or(true),
        BigIntValidator::BetweenBigInt(lo, hi) => match (parse(lo), parse(hi)) {
            (Some(lo), Some(hi)) => n >= lo && n <= hi,
            _ => true,
        },
        BigIntValidator::PositiveBigInt => n > 0,
        BigIntValidator::NonNegativeBigInt => n >= 0,
        BigIntValidator::NegativeBigInt => n < 0,
        BigIntValidator::NonPositiveBigInt => n <= 0,
    }
}

fn match_bigdecimal_validator(bv: &BigDecimalValidator, s: &str) -> bool {
    let n: f64 = match s.parse() {
        Ok(n) => n,
        Err(_) => return true,
    };
    let parse = |x: &str| x.parse::<f64>().ok();
    match bv {
        BigDecimalValidator::GreaterThanBigDecimal(b) => parse(b).map(|b| n > b).unwrap_or(true),
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(b) => {
            parse(b).map(|b| n >= b).unwrap_or(true)
        }
        BigDecimalValidator::LessThanBigDecimal(b) => parse(b).map(|b| n < b).unwrap_or(true),
        BigDecimalValidator::LessThanOrEqualToBigDecimal(b) => {
            parse(b).map(|b| n <= b).unwrap_or(true)
        }
        BigDecimalValidator::BetweenBigDecimal(lo, hi) => match (parse(lo), parse(hi)) {
            (Some(lo), Some(hi)) => n >= lo && n <= hi,
            _ => true,
        },
        BigDecimalValidator::PositiveBigDecimal => n > 0.0,
        BigDecimalValidator::NonNegativeBigDecimal => n >= 0.0,
        BigDecimalValidator::NegativeBigDecimal => n < 0.0,
        BigDecimalValidator::NonPositiveBigDecimal => n <= 0.0,
    }
}

fn match_duration_validator(dv: &DurationValidator, n: i128) -> bool {
    match dv {
        DurationValidator::GreaterThanDuration(s) => parse_duration_to_nanos(s)
            .map(|other| n > other)
            .unwrap_or(true),
        DurationValidator::GreaterThanOrEqualToDuration(s) => parse_duration_to_nanos(s)
            .map(|other| n >= other)
            .unwrap_or(true),
        DurationValidator::LessThanDuration(s) => parse_duration_to_nanos(s)
            .map(|other| n < other)
            .unwrap_or(true),
        DurationValidator::LessThanOrEqualToDuration(s) => parse_duration_to_nanos(s)
            .map(|other| n <= other)
            .unwrap_or(true),
        DurationValidator::BetweenDuration(start, end) => {
            match (parse_duration_to_nanos(start), parse_duration_to_nanos(end)) {
                (Some(lo), Some(hi)) => n >= lo && n <= hi,
                _ => true,
            }
        }
    }
}

impl ToTokens for Validator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            Validator::StringValidator(v) => {
                quote! { ::evenframe::validator::Validator::StringValidator(#v) }
            }
            Validator::NumberValidator(v) => {
                quote! { ::evenframe::validator::Validator::NumberValidator(#v) }
            }
            Validator::ArrayValidator(v) => {
                quote! { ::evenframe::validator::Validator::ArrayValidator(#v) }
            }
            Validator::DateValidator(v) => {
                quote! { ::evenframe::validator::Validator::DateValidator(#v) }
            }
            Validator::BigIntValidator(v) => {
                quote! { ::evenframe::validator::Validator::BigIntValidator(#v) }
            }
            Validator::BigDecimalValidator(v) => {
                quote! { ::evenframe::validator::Validator::BigDecimalValidator(#v) }
            }
            Validator::DurationValidator(v) => {
                quote! { ::evenframe::validator::Validator::DurationValidator(#v) }
            }
        };
        tokens.extend(variant_tokens);
    }
}

impl ToTokens for StringValidator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            StringValidator::String => {
                quote! { ::evenframe::validator::StringValidator::String }
            }
            StringValidator::Alpha => {
                quote! { ::evenframe::validator::StringValidator::Alpha }
            }
            StringValidator::Alphanumeric => {
                quote! { ::evenframe::validator::StringValidator::Alphanumeric }
            }
            StringValidator::Base64 => {
                quote! { ::evenframe::validator::StringValidator::Base64 }
            }
            StringValidator::Base64Url => {
                quote! { ::evenframe::validator::StringValidator::Base64Url }
            }
            StringValidator::Capitalize => {
                quote! { ::evenframe::validator::StringValidator::Capitalize }
            }
            StringValidator::CapitalizePreformatted => {
                quote! { ::evenframe::validator::StringValidator::CapitalizePreformatted }
            }
            StringValidator::CreditCard => {
                quote! { ::evenframe::validator::StringValidator::CreditCard }
            }
            StringValidator::Date => {
                quote! { ::evenframe::validator::StringValidator::Date }
            }
            StringValidator::DateEpoch => {
                quote! { ::evenframe::validator::StringValidator::DateEpoch }
            }
            StringValidator::DateEpochParse => {
                quote! { ::evenframe::validator::StringValidator::DateEpochParse }
            }
            StringValidator::DateIso => {
                quote! { ::evenframe::validator::StringValidator::DateIso }
            }
            StringValidator::DateIsoParse => {
                quote! { ::evenframe::validator::StringValidator::DateIsoParse }
            }
            StringValidator::DateParse => {
                quote! { ::evenframe::validator::StringValidator::DateParse }
            }
            StringValidator::Digits => {
                quote! { ::evenframe::validator::StringValidator::Digits }
            }
            StringValidator::Email => {
                quote! { ::evenframe::validator::StringValidator::Email }
            }
            StringValidator::Hex => {
                quote! { ::evenframe::validator::StringValidator::Hex }
            }
            StringValidator::Integer => {
                quote! { ::evenframe::validator::StringValidator::Integer }
            }
            StringValidator::IntegerParse => {
                quote! { ::evenframe::validator::StringValidator::IntegerParse }
            }
            StringValidator::Ip => quote! { ::evenframe::validator::StringValidator::Ip },
            StringValidator::IpV4 => {
                quote! { ::evenframe::validator::StringValidator::IpV4 }
            }
            StringValidator::IpV6 => {
                quote! { ::evenframe::validator::StringValidator::IpV6 }
            }
            StringValidator::Json => {
                quote! { ::evenframe::validator::StringValidator::Json }
            }
            StringValidator::JsonParse => {
                quote! { ::evenframe::validator::StringValidator::JsonParse }
            }
            StringValidator::Lower => {
                quote! { ::evenframe::validator::StringValidator::Lower }
            }
            StringValidator::LowerPreformatted => {
                quote! { ::evenframe::validator::StringValidator::LowerPreformatted }
            }
            StringValidator::Normalize => {
                quote! { ::evenframe::validator::StringValidator::Normalize }
            }
            StringValidator::NormalizeNFC => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFC }
            }
            StringValidator::NormalizeNFCPreformatted => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFCPreformatted }
            }
            StringValidator::NormalizeNFD => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFD }
            }
            StringValidator::NormalizeNFDPreformatted => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFDPreformatted }
            }
            StringValidator::NormalizeNFKC => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFKC }
            }
            StringValidator::NormalizeNFKCPreformatted => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFKCPreformatted }
            }
            StringValidator::NormalizeNFKD => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFKD }
            }
            StringValidator::NormalizeNFKDPreformatted => {
                quote! { ::evenframe::validator::StringValidator::NormalizeNFKDPreformatted }
            }
            StringValidator::Numeric => {
                quote! { ::evenframe::validator::StringValidator::Numeric }
            }
            StringValidator::NumericParse => {
                quote! { ::evenframe::validator::StringValidator::NumericParse }
            }
            StringValidator::Regex => {
                quote! { ::evenframe::validator::StringValidator::Regex }
            }
            StringValidator::Semver => {
                quote! { ::evenframe::validator::StringValidator::Semver }
            }
            StringValidator::Trim => {
                quote! { ::evenframe::validator::StringValidator::Trim }
            }
            StringValidator::TrimPreformatted => {
                quote! { ::evenframe::validator::StringValidator::TrimPreformatted }
            }
            StringValidator::Upper => {
                quote! { ::evenframe::validator::StringValidator::Upper }
            }
            StringValidator::UpperPreformatted => {
                quote! { ::evenframe::validator::StringValidator::UpperPreformatted }
            }
            StringValidator::Url => {
                quote! { ::evenframe::validator::StringValidator::Url }
            }
            StringValidator::UrlParse => {
                quote! { ::evenframe::validator::StringValidator::UrlParse }
            }
            StringValidator::Uuid => {
                quote! { ::evenframe::validator::StringValidator::Uuid }
            }
            StringValidator::UuidV1 => {
                quote! { ::evenframe::validator::StringValidator::UuidV1 }
            }
            StringValidator::UuidV2 => {
                quote! { ::evenframe::validator::StringValidator::UuidV2 }
            }
            StringValidator::UuidV3 => {
                quote! { ::evenframe::validator::StringValidator::UuidV3 }
            }
            StringValidator::UuidV4 => {
                quote! { ::evenframe::validator::StringValidator::UuidV4 }
            }
            StringValidator::UuidV5 => {
                quote! { ::evenframe::validator::StringValidator::UuidV5 }
            }
            StringValidator::UuidV6 => {
                quote! { ::evenframe::validator::StringValidator::UuidV6 }
            }
            StringValidator::UuidV7 => {
                quote! { ::evenframe::validator::StringValidator::UuidV7 }
            }
            StringValidator::UuidV8 => {
                quote! { ::evenframe::validator::StringValidator::UuidV8 }
            }
            StringValidator::Literal(s) => {
                quote! { ::evenframe::validator::StringValidator::Literal(#s.to_string()) }
            }
            StringValidator::StringEmbedded(s) => {
                quote! { ::evenframe::validator::StringValidator::StringEmbedded(#s.to_string()) }
            }
            StringValidator::RegexLiteral(f) => {
                quote! { ::evenframe::validator::StringValidator::RegexLiteral(#f) }
            }
            StringValidator::Length(s) => {
                quote! { ::evenframe::validator::StringValidator::Length(#s.to_string()) }
            }
            StringValidator::MinLength(n) => {
                quote! { ::evenframe::validator::StringValidator::MinLength(#n) }
            }
            StringValidator::MaxLength(n) => {
                quote! { ::evenframe::validator::StringValidator::MaxLength(#n) }
            }
            StringValidator::NonEmpty => {
                quote! { ::evenframe::validator::StringValidator::NonEmpty }
            }
            StringValidator::StartsWith(s) => {
                quote! { ::evenframe::validator::StringValidator::StartsWith(#s.to_string()) }
            }
            StringValidator::EndsWith(s) => {
                quote! { ::evenframe::validator::StringValidator::EndsWith(#s.to_string()) }
            }
            StringValidator::Includes(s) => {
                quote! { ::evenframe::validator::StringValidator::Includes(#s.to_string()) }
            }
            StringValidator::Trimmed => {
                quote! { ::evenframe::validator::StringValidator::Trimmed }
            }
            StringValidator::Lowercased => {
                quote! { ::evenframe::validator::StringValidator::Lowercased }
            }
            StringValidator::Uppercased => {
                quote! { ::evenframe::validator::StringValidator::Uppercased }
            }
            StringValidator::Capitalized => {
                quote! { ::evenframe::validator::StringValidator::Capitalized }
            }
            StringValidator::Uncapitalized => {
                quote! { ::evenframe::validator::StringValidator::Uncapitalized }
            }
        };
        tokens.extend(variant_tokens);
    }
}

impl ToTokens for NumberValidator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            NumberValidator::GreaterThan(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::GreaterThan(::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::GreaterThanOrEqualTo(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::GreaterThanOrEqualTo(::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::LessThan(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::LessThan(::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::LessThanOrEqualTo(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::LessThanOrEqualTo(::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::Between(start, end) => {
                let s = start.0;
                let e = end.0;
                quote! { ::evenframe::validator::NumberValidator::Between(::ordered_float::OrderedFloat(#s), ::ordered_float::OrderedFloat(#e)) }
            }
            NumberValidator::Int => {
                quote! { ::evenframe::validator::NumberValidator::Int }
            }
            NumberValidator::NonNaN => {
                quote! { ::evenframe::validator::NumberValidator::NonNaN }
            }
            NumberValidator::Positive => {
                quote! { ::evenframe::validator::NumberValidator::Positive }
            }
            NumberValidator::Negative => {
                quote! { ::evenframe::validator::NumberValidator::Negative }
            }
            NumberValidator::NonPositive => {
                quote! { ::evenframe::validator::NumberValidator::NonPositive }
            }
            NumberValidator::NonNegative => {
                quote! { ::evenframe::validator::NumberValidator::NonNegative }
            }
            NumberValidator::Finite => {
                quote! { ::evenframe::validator::NumberValidator::Finite }
            }
            NumberValidator::MultipleOf(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::MultipleOf(::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::Uint8 => {
                quote! { ::evenframe::validator::NumberValidator::Uint8 }
            }
        };
        tokens.extend(variant_tokens);
    }
}

impl ToTokens for ArrayValidator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            ArrayValidator::MinItems(n) => {
                quote! { ::evenframe::validator::ArrayValidator::MinItems(#n) }
            }
            ArrayValidator::MaxItems(n) => {
                quote! { ::evenframe::validator::ArrayValidator::MaxItems(#n) }
            }
            ArrayValidator::ItemsCount(n) => {
                quote! { ::evenframe::validator::ArrayValidator::ItemsCount(#n) }
            }
        };
        tokens.extend(variant_tokens);
    }
}

impl ToTokens for DateValidator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            DateValidator::ValidDate => {
                quote! { ::evenframe::validator::DateValidator::ValidDate }
            }
            DateValidator::GreaterThanDate(s) => {
                quote! { ::evenframe::validator::DateValidator::GreaterThanDate(#s.to_string()) }
            }
            DateValidator::GreaterThanOrEqualToDate(s) => {
                quote! { ::evenframe::validator::DateValidator::GreaterThanOrEqualToDate(#s.to_string()) }
            }
            DateValidator::LessThanDate(s) => {
                quote! { ::evenframe::validator::DateValidator::LessThanDate(#s.to_string()) }
            }
            DateValidator::LessThanOrEqualToDate(s) => {
                quote! { ::evenframe::validator::DateValidator::LessThanOrEqualToDate(#s.to_string()) }
            }
            DateValidator::BetweenDate(start, end) => {
                quote! { ::evenframe::validator::DateValidator::BetweenDate(#start.to_string(), #end.to_string()) }
            }
        };
        tokens.extend(variant_tokens);
    }
}

impl ToTokens for BigIntValidator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            BigIntValidator::GreaterThanBigInt(s) => {
                quote! { ::evenframe::validator::BigIntValidator::GreaterThanBigInt(#s.to_string()) }
            }
            BigIntValidator::GreaterThanOrEqualToBigInt(s) => {
                quote! { ::evenframe::validator::BigIntValidator::GreaterThanOrEqualToBigInt(#s.to_string()) }
            }
            BigIntValidator::LessThanBigInt(s) => {
                quote! { ::evenframe::validator::BigIntValidator::LessThanBigInt(#s.to_string()) }
            }
            BigIntValidator::LessThanOrEqualToBigInt(s) => {
                quote! { ::evenframe::validator::BigIntValidator::LessThanOrEqualToBigInt(#s.to_string()) }
            }
            BigIntValidator::BetweenBigInt(start, end) => {
                quote! { ::evenframe::validator::BigIntValidator::BetweenBigInt(#start.to_string(), #end.to_string()) }
            }
            BigIntValidator::PositiveBigInt => {
                quote! { ::evenframe::validator::BigIntValidator::PositiveBigInt }
            }
            BigIntValidator::NegativeBigInt => {
                quote! { ::evenframe::validator::BigIntValidator::NegativeBigInt }
            }
            BigIntValidator::NonPositiveBigInt => {
                quote! { ::evenframe::validator::BigIntValidator::NonPositiveBigInt }
            }
            BigIntValidator::NonNegativeBigInt => {
                quote! { ::evenframe::validator::BigIntValidator::NonNegativeBigInt }
            }
        };
        tokens.extend(variant_tokens);
    }
}

impl ToTokens for BigDecimalValidator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            BigDecimalValidator::GreaterThanBigDecimal(s) => {
                quote! { ::evenframe::validator::BigDecimalValidator::GreaterThanBigDecimal(#s.to_string()) }
            }
            BigDecimalValidator::GreaterThanOrEqualToBigDecimal(s) => {
                quote! { ::evenframe::validator::BigDecimalValidator::GreaterThanOrEqualToBigDecimal(#s.to_string()) }
            }
            BigDecimalValidator::LessThanBigDecimal(s) => {
                quote! { ::evenframe::validator::BigDecimalValidator::LessThanBigDecimal(#s.to_string()) }
            }
            BigDecimalValidator::LessThanOrEqualToBigDecimal(s) => {
                quote! { ::evenframe::validator::BigDecimalValidator::LessThanOrEqualToBigDecimal(#s.to_string()) }
            }
            BigDecimalValidator::BetweenBigDecimal(start, end) => {
                quote! { ::evenframe::validator::BigDecimalValidator::BetweenBigDecimal(#start.to_string(), #end.to_string()) }
            }
            BigDecimalValidator::PositiveBigDecimal => {
                quote! { ::evenframe::validator::BigDecimalValidator::PositiveBigDecimal }
            }
            BigDecimalValidator::NegativeBigDecimal => {
                quote! { ::evenframe::validator::BigDecimalValidator::NegativeBigDecimal }
            }
            BigDecimalValidator::NonPositiveBigDecimal => {
                quote! { ::evenframe::validator::BigDecimalValidator::NonPositiveBigDecimal }
            }
            BigDecimalValidator::NonNegativeBigDecimal => {
                quote! { ::evenframe::validator::BigDecimalValidator::NonNegativeBigDecimal }
            }
        };
        tokens.extend(variant_tokens);
    }
}

impl ToTokens for DurationValidator {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            DurationValidator::GreaterThanDuration(s) => {
                quote! { ::evenframe::validator::DurationValidator::GreaterThanDuration(#s.to_string()) }
            }
            DurationValidator::GreaterThanOrEqualToDuration(s) => {
                quote! { ::evenframe::validator::DurationValidator::GreaterThanOrEqualToDuration(#s.to_string()) }
            }
            DurationValidator::LessThanDuration(s) => {
                quote! { ::evenframe::validator::DurationValidator::LessThanDuration(#s.to_string()) }
            }
            DurationValidator::LessThanOrEqualToDuration(s) => {
                quote! { ::evenframe::validator::DurationValidator::LessThanOrEqualToDuration(#s.to_string()) }
            }
            DurationValidator::BetweenDuration(start, end) => {
                quote! { ::evenframe::validator::DurationValidator::BetweenDuration(#start.to_string(), #end.to_string()) }
            }
        };
        tokens.extend(variant_tokens);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;

    // ==================== Validator Enum Tests ====================

    #[test]
    fn test_validator_from_string_validator() {
        let sv = StringValidator::Email;
        let v: Validator = sv.into();
        assert!(matches!(
            v,
            Validator::StringValidator(StringValidator::Email)
        ));
    }

    #[test]
    fn test_validator_from_number_validator() {
        let nv = NumberValidator::Positive;
        let v: Validator = nv.into();
        assert!(matches!(
            v,
            Validator::NumberValidator(NumberValidator::Positive)
        ));
    }

    #[test]
    fn test_validator_from_array_validator() {
        let av = ArrayValidator::MinItems(5);
        let v: Validator = av.into();
        assert!(matches!(
            v,
            Validator::ArrayValidator(ArrayValidator::MinItems(5))
        ));
    }

    #[test]
    fn test_validator_from_date_validator() {
        let dv = DateValidator::ValidDate;
        let v: Validator = dv.into();
        assert!(matches!(
            v,
            Validator::DateValidator(DateValidator::ValidDate)
        ));
    }

    #[test]
    fn test_validator_from_bigint_validator() {
        let bv = BigIntValidator::PositiveBigInt;
        let v: Validator = bv.into();
        assert!(matches!(
            v,
            Validator::BigIntValidator(BigIntValidator::PositiveBigInt)
        ));
    }

    #[test]
    fn test_validator_from_bigdecimal_validator() {
        let bv = BigDecimalValidator::PositiveBigDecimal;
        let v: Validator = bv.into();
        assert!(matches!(
            v,
            Validator::BigDecimalValidator(BigDecimalValidator::PositiveBigDecimal)
        ));
    }

    #[test]
    fn test_validator_from_duration_validator() {
        let dv = DurationValidator::GreaterThanDuration("1h".to_string());
        let v: Validator = dv.into();
        assert!(matches!(
            v,
            Validator::DurationValidator(DurationValidator::GreaterThanDuration(_))
        ));
    }

    // ==================== StringValidator Tests ====================

    #[test]
    fn test_string_validator_equality() {
        assert_eq!(StringValidator::Email, StringValidator::Email);
        assert_ne!(StringValidator::Email, StringValidator::Url);
    }

    #[test]
    fn test_string_validator_with_parameters() {
        let v1 = StringValidator::MinLength(5);
        let v2 = StringValidator::MinLength(5);
        let v3 = StringValidator::MinLength(10);

        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
    }

    #[test]
    fn test_string_validator_literal() {
        let v = StringValidator::Literal("hello".to_string());
        assert!(matches!(v, StringValidator::Literal(s) if s == "hello"));
    }

    #[test]
    fn test_string_validator_starts_with() {
        let v = StringValidator::StartsWith("prefix".to_string());
        assert!(matches!(v, StringValidator::StartsWith(s) if s == "prefix"));
    }

    #[test]
    fn test_string_validator_ends_with() {
        let v = StringValidator::EndsWith("suffix".to_string());
        assert!(matches!(v, StringValidator::EndsWith(s) if s == "suffix"));
    }

    #[test]
    fn test_string_validator_includes() {
        let v = StringValidator::Includes("substring".to_string());
        assert!(matches!(v, StringValidator::Includes(s) if s == "substring"));
    }

    #[test]
    fn test_string_validator_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(StringValidator::Email);
        set.insert(StringValidator::Url);
        set.insert(StringValidator::Email); // duplicate
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_string_validator_clone() {
        let v = StringValidator::MinLength(10);
        let cloned = v.clone();
        assert_eq!(v, cloned);
    }

    // ==================== NumberValidator Tests ====================

    #[test]
    fn test_number_validator_greater_than() {
        let v = NumberValidator::GreaterThan(OrderedFloat(5.0));
        assert!(matches!(v, NumberValidator::GreaterThan(OrderedFloat(x)) if x == 5.0));
    }

    #[test]
    fn test_number_validator_less_than() {
        let v = NumberValidator::LessThan(OrderedFloat(10.0));
        assert!(matches!(v, NumberValidator::LessThan(OrderedFloat(x)) if x == 10.0));
    }

    #[test]
    fn test_number_validator_between() {
        let v = NumberValidator::Between(OrderedFloat(1.0), OrderedFloat(10.0));
        assert!(
            matches!(v, NumberValidator::Between(OrderedFloat(a), OrderedFloat(b)) if a == 1.0 && b == 10.0)
        );
    }

    #[test]
    fn test_number_validator_multiple_of() {
        let v = NumberValidator::MultipleOf(OrderedFloat(3.0));
        assert!(matches!(v, NumberValidator::MultipleOf(OrderedFloat(x)) if x == 3.0));
    }

    #[test]
    fn test_number_validator_equality() {
        assert_eq!(NumberValidator::Positive, NumberValidator::Positive);
        assert_ne!(NumberValidator::Positive, NumberValidator::Negative);
    }

    #[test]
    fn test_number_validator_int() {
        let v = NumberValidator::Int;
        assert!(matches!(v, NumberValidator::Int));
    }

    #[test]
    fn test_number_validator_finite() {
        let v = NumberValidator::Finite;
        assert!(matches!(v, NumberValidator::Finite));
    }

    #[test]
    fn test_number_validator_uint8() {
        let v = NumberValidator::Uint8;
        assert!(matches!(v, NumberValidator::Uint8));
    }

    // ==================== ArrayValidator Tests ====================

    #[test]
    fn test_array_validator_min_items() {
        let v = ArrayValidator::MinItems(3);
        assert!(matches!(v, ArrayValidator::MinItems(3)));
    }

    #[test]
    fn test_array_validator_max_items() {
        let v = ArrayValidator::MaxItems(10);
        assert!(matches!(v, ArrayValidator::MaxItems(10)));
    }

    #[test]
    fn test_array_validator_items_count() {
        let v = ArrayValidator::ItemsCount(5);
        assert!(matches!(v, ArrayValidator::ItemsCount(5)));
    }

    #[test]
    fn test_array_validator_equality() {
        assert_eq!(ArrayValidator::MinItems(5), ArrayValidator::MinItems(5));
        assert_ne!(ArrayValidator::MinItems(5), ArrayValidator::MinItems(10));
        assert_ne!(ArrayValidator::MinItems(5), ArrayValidator::MaxItems(5));
    }

    // ==================== DateValidator Tests ====================

    #[test]
    fn test_date_validator_valid_date() {
        let v = DateValidator::ValidDate;
        assert!(matches!(v, DateValidator::ValidDate));
    }

    #[test]
    fn test_date_validator_greater_than() {
        let v = DateValidator::GreaterThanDate("2024-01-01".to_string());
        assert!(matches!(v, DateValidator::GreaterThanDate(s) if s == "2024-01-01"));
    }

    #[test]
    fn test_date_validator_between() {
        let v = DateValidator::BetweenDate("2024-01-01".to_string(), "2024-12-31".to_string());
        assert!(
            matches!(v, DateValidator::BetweenDate(start, end) if start == "2024-01-01" && end == "2024-12-31")
        );
    }

    // ==================== BigIntValidator Tests ====================

    #[test]
    fn test_bigint_validator_greater_than() {
        let v = BigIntValidator::GreaterThanBigInt("1000000000000".to_string());
        assert!(matches!(v, BigIntValidator::GreaterThanBigInt(s) if s == "1000000000000"));
    }

    #[test]
    fn test_bigint_validator_positive() {
        let v = BigIntValidator::PositiveBigInt;
        assert!(matches!(v, BigIntValidator::PositiveBigInt));
    }

    #[test]
    fn test_bigint_validator_between() {
        let v = BigIntValidator::BetweenBigInt("0".to_string(), "100".to_string());
        assert!(
            matches!(v, BigIntValidator::BetweenBigInt(start, end) if start == "0" && end == "100")
        );
    }

    // ==================== BigDecimalValidator Tests ====================

    #[test]
    fn test_bigdecimal_validator_greater_than() {
        let v = BigDecimalValidator::GreaterThanBigDecimal("0.001".to_string());
        assert!(matches!(v, BigDecimalValidator::GreaterThanBigDecimal(s) if s == "0.001"));
    }

    #[test]
    fn test_bigdecimal_validator_positive() {
        let v = BigDecimalValidator::PositiveBigDecimal;
        assert!(matches!(v, BigDecimalValidator::PositiveBigDecimal));
    }

    // ==================== DurationValidator Tests ====================

    #[test]
    fn test_duration_validator_greater_than() {
        let v = DurationValidator::GreaterThanDuration("1h".to_string());
        assert!(matches!(v, DurationValidator::GreaterThanDuration(s) if s == "1h"));
    }

    #[test]
    fn test_duration_validator_between() {
        let v = DurationValidator::BetweenDuration("1m".to_string(), "1h".to_string());
        assert!(
            matches!(v, DurationValidator::BetweenDuration(start, end) if start == "1m" && end == "1h")
        );
    }

    // ==================== Serialization Tests ====================

    #[test]
    fn test_validator_serialize_deserialize() {
        let v = Validator::StringValidator(StringValidator::Email);
        let json = serde_json::to_string(&v).unwrap();
        let deserialized: Validator = serde_json::from_str(&json).unwrap();
        assert_eq!(v, deserialized);
    }

    #[test]
    fn test_string_validator_serialize_deserialize() {
        let v = StringValidator::MinLength(10);
        let json = serde_json::to_string(&v).unwrap();
        let deserialized: StringValidator = serde_json::from_str(&json).unwrap();
        assert_eq!(v, deserialized);
    }

    #[test]
    fn test_number_validator_serialize_deserialize() {
        let v = NumberValidator::GreaterThan(OrderedFloat(5.5));
        let json = serde_json::to_string(&v).unwrap();
        let deserialized: NumberValidator = serde_json::from_str(&json).unwrap();
        assert_eq!(v, deserialized);
    }

    #[test]
    fn test_array_validator_serialize_deserialize() {
        let v = ArrayValidator::MinItems(5);
        let json = serde_json::to_string(&v).unwrap();
        let deserialized: ArrayValidator = serde_json::from_str(&json).unwrap();
        assert_eq!(v, deserialized);
    }

    // ==================== ToTokens Tests ====================

    #[test]
    fn test_validator_to_tokens_not_empty() {
        let v = Validator::StringValidator(StringValidator::Email);
        let tokens = v.to_token_stream();
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_string_validator_to_tokens() {
        let v = StringValidator::Alpha;
        let tokens = v.to_token_stream();
        let token_string = tokens.to_string();
        assert!(token_string.contains("Alpha"));
    }

    #[test]
    fn test_number_validator_to_tokens() {
        let v = NumberValidator::Positive;
        let tokens = v.to_token_stream();
        let token_string = tokens.to_string();
        assert!(token_string.contains("Positive"));
    }

    #[test]
    fn test_array_validator_to_tokens() {
        let v = ArrayValidator::MaxItems(10);
        let tokens = v.to_token_stream();
        let token_string = tokens.to_string();
        assert!(token_string.contains("MaxItems"));
    }

    #[test]
    fn test_date_validator_to_tokens() {
        let v = DateValidator::ValidDate;
        let tokens = v.to_token_stream();
        let token_string = tokens.to_string();
        assert!(token_string.contains("ValidDate"));
    }

    #[test]
    fn test_bigint_validator_to_tokens() {
        let v = BigIntValidator::PositiveBigInt;
        let tokens = v.to_token_stream();
        let token_string = tokens.to_string();
        assert!(token_string.contains("PositiveBigInt"));
    }

    #[test]
    fn test_bigdecimal_validator_to_tokens() {
        let v = BigDecimalValidator::NegativeBigDecimal;
        let tokens = v.to_token_stream();
        let token_string = tokens.to_string();
        assert!(token_string.contains("NegativeBigDecimal"));
    }

    #[test]
    fn test_duration_validator_to_tokens() {
        let v = DurationValidator::LessThanDuration("2h".to_string());
        let tokens = v.to_token_stream();
        let token_string = tokens.to_string();
        assert!(token_string.contains("LessThanDuration"));
    }

    // ==================== get_validation_logic_tokens Tests ====================

    #[test]
    fn test_get_validation_logic_tokens_alpha() {
        let v = Validator::StringValidator(StringValidator::Alpha);
        let tokens = v.get_validation_logic_tokens("value");
        let token_string = tokens.to_string();
        assert!(token_string.contains("alphabetic"));
    }

    #[test]
    fn test_get_validation_logic_tokens_email() {
        let v = Validator::StringValidator(StringValidator::Email);
        let tokens = v.get_validation_logic_tokens("email");
        let token_string = tokens.to_string();
        assert!(token_string.contains("email"));
    }

    #[test]
    fn test_get_validation_logic_tokens_min_length() {
        let v = Validator::StringValidator(StringValidator::MinLength(5));
        let tokens = v.get_validation_logic_tokens("text");
        let token_string = tokens.to_string();
        assert!(token_string.contains("len"));
    }

    #[test]
    fn test_get_validation_logic_tokens_positive_number() {
        let v = Validator::NumberValidator(NumberValidator::Positive);
        let tokens = v.get_validation_logic_tokens("num");
        let token_string = tokens.to_string();
        assert!(token_string.contains("positive"));
    }

    #[test]
    fn test_get_validation_logic_tokens_array_min_items() {
        let v = Validator::ArrayValidator(ArrayValidator::MinItems(3));
        let tokens = v.get_validation_logic_tokens("arr");
        let token_string = tokens.to_string();
        assert!(token_string.contains("len"));
    }

    #[test]
    fn test_get_validation_logic_tokens_uuid() {
        let v = Validator::StringValidator(StringValidator::Uuid);
        let tokens = v.get_validation_logic_tokens("id");
        let token_string = tokens.to_string();
        assert!(token_string.contains("uuid"));
    }

    // ==================== Debug Tests ====================

    #[test]
    fn test_validator_debug() {
        let v = Validator::StringValidator(StringValidator::Email);
        let debug_str = format!("{:?}", v);
        assert!(debug_str.contains("Email"));
    }

    #[test]
    fn test_string_validator_debug() {
        let v = StringValidator::Url;
        let debug_str = format!("{:?}", v);
        assert!(debug_str.contains("Url"));
    }

    #[test]
    fn test_number_validator_debug() {
        let v = NumberValidator::Between(OrderedFloat(1.0), OrderedFloat(10.0));
        let debug_str = format!("{:?}", v);
        assert!(debug_str.contains("Between"));
    }

    // ==================== Hash Tests ====================

    #[test]
    fn test_validator_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Validator::StringValidator(StringValidator::Email));
        set.insert(Validator::StringValidator(StringValidator::Url));
        set.insert(Validator::NumberValidator(NumberValidator::Positive));
        assert_eq!(set.len(), 3);
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_string_validator_empty_literal() {
        let v = StringValidator::Literal("".to_string());
        assert!(matches!(v, StringValidator::Literal(s) if s.is_empty()));
    }

    #[test]
    fn test_string_validator_zero_length() {
        let v = StringValidator::MinLength(0);
        assert!(matches!(v, StringValidator::MinLength(0)));
    }

    #[test]
    fn test_number_validator_zero() {
        let v = NumberValidator::GreaterThan(OrderedFloat(0.0));
        assert!(matches!(v, NumberValidator::GreaterThan(OrderedFloat(x)) if x == 0.0));
    }

    #[test]
    fn test_number_validator_negative() {
        let v = NumberValidator::LessThan(OrderedFloat(-5.0));
        assert!(matches!(v, NumberValidator::LessThan(OrderedFloat(x)) if x == -5.0));
    }

    #[test]
    fn test_array_validator_zero_items() {
        let v = ArrayValidator::MinItems(0);
        assert!(matches!(v, ArrayValidator::MinItems(0)));
    }

    // ==================== Validator::matches Tests ====================

    #[test]
    fn matches_string_min_length() {
        let v = Validator::StringValidator(StringValidator::MinLength(5));
        assert!(v.matches(&MockValue::Str("hello!")));
        assert!(v.matches(&MockValue::Str("12345")));
        assert!(!v.matches(&MockValue::Str("nope")));
    }

    #[test]
    fn matches_string_max_length() {
        let v = Validator::StringValidator(StringValidator::MaxLength(3));
        assert!(v.matches(&MockValue::Str("abc")));
        assert!(!v.matches(&MockValue::Str("abcd")));
    }

    #[test]
    fn matches_string_email() {
        let v = Validator::StringValidator(StringValidator::Email);
        assert!(v.matches(&MockValue::Str("user@example.com")));
        assert!(!v.matches(&MockValue::Str("not-an-email")));
        assert!(!v.matches(&MockValue::Str("user@nodot")));
    }

    #[test]
    fn matches_string_uuid() {
        let v = Validator::StringValidator(StringValidator::Uuid);
        assert!(v.matches(&MockValue::Str("550e8400-e29b-41d4-a716-446655440000")));
        assert!(!v.matches(&MockValue::Str("not-a-uuid")));
    }

    #[test]
    fn matches_string_starts_ends_includes() {
        let starts = Validator::StringValidator(StringValidator::StartsWith("foo".into()));
        assert!(starts.matches(&MockValue::Str("foobar")));
        assert!(!starts.matches(&MockValue::Str("barfoo")));

        let ends = Validator::StringValidator(StringValidator::EndsWith(".com".into()));
        assert!(ends.matches(&MockValue::Str("hi.com")));
        assert!(!ends.matches(&MockValue::Str("hi.org")));

        let inc = Validator::StringValidator(StringValidator::Includes("zz".into()));
        assert!(inc.matches(&MockValue::Str("buzzy")));
        assert!(!inc.matches(&MockValue::Str("plain")));
    }

    #[test]
    fn matches_string_lowercased_uppercased_capitalized() {
        let lower = Validator::StringValidator(StringValidator::Lowercased);
        assert!(lower.matches(&MockValue::Str("hello")));
        assert!(!lower.matches(&MockValue::Str("Hello")));

        let upper = Validator::StringValidator(StringValidator::Uppercased);
        assert!(upper.matches(&MockValue::Str("HELLO")));
        assert!(!upper.matches(&MockValue::Str("Hello")));

        let cap = Validator::StringValidator(StringValidator::Capitalized);
        assert!(cap.matches(&MockValue::Str("Hello")));
        assert!(!cap.matches(&MockValue::Str("hello")));
    }

    #[test]
    fn matches_number_between_and_positive() {
        let between = Validator::NumberValidator(NumberValidator::Between(
            OrderedFloat(1.0),
            OrderedFloat(10.0),
        ));
        assert!(between.matches(&MockValue::Num(5.0)));
        assert!(between.matches(&MockValue::Num(1.0)));
        assert!(between.matches(&MockValue::Num(10.0)));
        assert!(!between.matches(&MockValue::Num(0.0)));
        assert!(!between.matches(&MockValue::Num(11.0)));

        let positive = Validator::NumberValidator(NumberValidator::Positive);
        assert!(positive.matches(&MockValue::Num(0.001)));
        assert!(!positive.matches(&MockValue::Num(0.0)));
        assert!(!positive.matches(&MockValue::Num(-1.0)));
    }

    #[test]
    fn matches_number_int_uint8_multiple_of() {
        let int_v = Validator::NumberValidator(NumberValidator::Int);
        assert!(int_v.matches(&MockValue::Num(42.0)));
        assert!(!int_v.matches(&MockValue::Num(3.5)));

        let uint8 = Validator::NumberValidator(NumberValidator::Uint8);
        assert!(uint8.matches(&MockValue::Num(0.0)));
        assert!(uint8.matches(&MockValue::Num(255.0)));
        assert!(!uint8.matches(&MockValue::Num(256.0)));
        assert!(!uint8.matches(&MockValue::Num(-1.0)));

        let mult = Validator::NumberValidator(NumberValidator::MultipleOf(OrderedFloat(5.0)));
        assert!(mult.matches(&MockValue::Num(15.0)));
        assert!(!mult.matches(&MockValue::Num(7.0)));
    }

    #[test]
    fn matches_array() {
        let min_v = Validator::ArrayValidator(ArrayValidator::MinItems(3));
        assert!(min_v.matches(&MockValue::ArrayLen(3)));
        assert!(!min_v.matches(&MockValue::ArrayLen(2)));

        let exact = Validator::ArrayValidator(ArrayValidator::ItemsCount(5));
        assert!(exact.matches(&MockValue::ArrayLen(5)));
        assert!(!exact.matches(&MockValue::ArrayLen(4)));
    }

    #[test]
    fn matches_date_between() {
        let v = Validator::DateValidator(DateValidator::BetweenDate(
            "2024-01-01".into(),
            "2024-12-31".into(),
        ));
        let inside = chrono::NaiveDate::from_ymd_opt(2024, 6, 1).unwrap();
        let before = chrono::NaiveDate::from_ymd_opt(2023, 12, 31).unwrap();
        let after = chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        assert!(v.matches(&MockValue::Date(inside)));
        assert!(!v.matches(&MockValue::Date(before)));
        assert!(!v.matches(&MockValue::Date(after)));
    }

    #[test]
    fn matches_bigint() {
        let v = Validator::BigIntValidator(BigIntValidator::BetweenBigInt("0".into(), "100".into()));
        assert!(v.matches(&MockValue::BigInt("50")));
        assert!(!v.matches(&MockValue::BigInt("101")));

        let pos = Validator::BigIntValidator(BigIntValidator::PositiveBigInt);
        assert!(pos.matches(&MockValue::BigInt("1")));
        assert!(!pos.matches(&MockValue::BigInt("0")));
    }

    #[test]
    fn matches_bigdecimal() {
        let v = Validator::BigDecimalValidator(BigDecimalValidator::PositiveBigDecimal);
        assert!(v.matches(&MockValue::BigDecimal("0.001")));
        assert!(!v.matches(&MockValue::BigDecimal("-0.001")));
    }

    #[test]
    fn matches_duration() {
        // 30 minutes between 1m and 1h: should match.
        let v = Validator::DurationValidator(DurationValidator::BetweenDuration(
            "1m".into(),
            "1h".into(),
        ));
        let thirty_min_ns: i128 = 30 * 60 * 1_000_000_000;
        assert!(v.matches(&MockValue::DurationNanos(thirty_min_ns)));
        let two_hours_ns: i128 = 2 * 60 * 60 * 1_000_000_000;
        assert!(!v.matches(&MockValue::DurationNanos(two_hours_ns)));
    }

    #[test]
    fn matches_validators_outside_their_domain_return_true() {
        // A NumberValidator on a string mock value is irrelevant — return true
        // so the retry loop doesn't reject perfectly fine string candidates
        // when the user mis-attached a validator.
        let nv = Validator::NumberValidator(NumberValidator::Positive);
        assert!(nv.matches(&MockValue::Str("hello")));

        let sv = Validator::StringValidator(StringValidator::Email);
        assert!(sv.matches(&MockValue::Num(42.0)));
    }

    #[test]
    fn parse_duration_to_nanos_basic() {
        assert_eq!(super::parse_duration_to_nanos("1s"), Some(1_000_000_000));
        assert_eq!(super::parse_duration_to_nanos("1m"), Some(60 * 1_000_000_000));
        assert_eq!(
            super::parse_duration_to_nanos("1h30m"),
            Some(90 * 60 * 1_000_000_000)
        );
        assert_eq!(super::parse_duration_to_nanos("500ms"), Some(500_000_000));
        assert_eq!(super::parse_duration_to_nanos("garbage"), None);
        assert_eq!(super::parse_duration_to_nanos(""), None);
    }
}
