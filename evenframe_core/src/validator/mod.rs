pub mod bounds;
pub mod keywords;
pub mod runtime;
pub mod string_rules;

use crate::schemasync::mockmake::format::Format;
use derive_more::From;
use ordered_float::OrderedFloat;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::{Deserialize, Serialize};
use string_rules::StringRule;
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

    /// A semantic version (see <https://semver.org/>)
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
    /// The deserializer statement that applies this validator to `place`, a
    /// mutable place holding the field's value. Parse morphs change how the
    /// field is read, so the derive applies them there instead.
    pub fn validation_tokens(
        &self,
        place: &TokenStream,
        field_name: &str,
    ) -> Result<TokenStream, String> {
        self.check_bounds()?;
        let (validator_type, validator_value, call) = match self {
            Validator::StringValidator(validator) => {
                let call = match validator.rule() {
                    StringRule::Check => quote! { check_string(&#place, &VALIDATOR) },
                    StringRule::Transform(_) => {
                        quote! { transform_string(&mut #place, &VALIDATOR) }
                    }
                    StringRule::Carrier => return Ok(TokenStream::new()),
                    StringRule::Parse(_) => {
                        return Err(format!(
                            "{validator:?} parses the field's input, so it must come first and only once"
                        ));
                    }
                };
                (quote! { StringValidator }, quote! { #validator }, call)
            }
            Validator::NumberValidator(validator) => (
                quote! { NumberValidator },
                quote! { #validator },
                quote! { check_number(&#place, &VALIDATOR) },
            ),
            Validator::ArrayValidator(validator) => (
                quote! { ArrayValidator },
                quote! { #validator },
                quote! { check_items(&#place, &VALIDATOR) },
            ),
            Validator::DateValidator(validator) => (
                quote! { DateValidator },
                quote! { #validator },
                quote! { check_date(&#place, &VALIDATOR) },
            ),
            Validator::BigIntValidator(validator) => (
                quote! { BigIntValidator },
                quote! { #validator },
                quote! { check_big_int(&#place, &VALIDATOR) },
            ),
            Validator::BigDecimalValidator(validator) => (
                quote! { BigDecimalValidator },
                quote! { #validator },
                quote! { check_decimal(&#place, &VALIDATOR) },
            ),
            Validator::DurationValidator(validator) => (
                quote! { DurationValidator },
                quote! { #validator },
                quote! { check_duration(&#place, &VALIDATOR) },
            ),
        };
        Ok(quote! {
            {
                static VALIDATOR: ::std::sync::LazyLock<::evenframe::validator::#validator_type> =
                    ::std::sync::LazyLock::new(|| #validator_value);
                ::evenframe::validator::runtime::#call.map_err(|rejection| {
                    ::serde::de::Error::custom(::std::format!("{}: {}", #field_name, rejection))
                })?;
            }
        })
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
    /// This validator in words, for messages about the values it accepts.
    pub fn describe(&self) -> String {
        match self {
            Validator::StringValidator(string) => string.expectation(),
            Validator::NumberValidator(number) => match number {
                NumberValidator::GreaterThan(value) => format!("a number above {}", value.0),
                NumberValidator::GreaterThanOrEqualTo(value) => {
                    format!("a number of at least {}", value.0)
                }
                NumberValidator::LessThan(value) => format!("a number below {}", value.0),
                NumberValidator::LessThanOrEqualTo(value) => {
                    format!("a number of at most {}", value.0)
                }
                NumberValidator::Between(start, end) => {
                    format!("a number from {} to {}", start.0, end.0)
                }
                NumberValidator::Positive => "a positive number".to_string(),
                NumberValidator::NonNegative => "a number of at least 0".to_string(),
                NumberValidator::Negative => "a negative number".to_string(),
                NumberValidator::NonPositive => "a number of at most 0".to_string(),
                NumberValidator::Int => "an integer".to_string(),
                NumberValidator::Uint8 => "an integer from 0 to 255".to_string(),
                NumberValidator::MultipleOf(divisor) => format!("a multiple of {}", divisor.0),
                NumberValidator::Finite => "a finite number".to_string(),
                NumberValidator::NonNaN => "a number that is not NaN".to_string(),
            },
            Validator::ArrayValidator(array) => match array {
                ArrayValidator::MinItems(count) => format!("at least {count} items"),
                ArrayValidator::MaxItems(count) => format!("at most {count} items"),
                ArrayValidator::ItemsCount(count) => format!("exactly {count} items"),
            },
            Validator::DateValidator(date) => match date {
                DateValidator::ValidDate => "a valid date".to_string(),
                DateValidator::GreaterThanDate(bound) => format!("a date after {bound}"),
                DateValidator::GreaterThanOrEqualToDate(bound) => {
                    format!("a date on or after {bound}")
                }
                DateValidator::LessThanDate(bound) => format!("a date before {bound}"),
                DateValidator::LessThanOrEqualToDate(bound) => {
                    format!("a date on or before {bound}")
                }
                DateValidator::BetweenDate(start, end) => format!("a date from {start} to {end}"),
            },
            Validator::BigIntValidator(big_int) => match big_int {
                BigIntValidator::PositiveBigInt => "a positive integer".to_string(),
                BigIntValidator::NegativeBigInt => "a negative integer".to_string(),
                BigIntValidator::NonNegativeBigInt => "an integer of at least 0".to_string(),
                BigIntValidator::NonPositiveBigInt => "an integer of at most 0".to_string(),
                BigIntValidator::GreaterThanBigInt(bound) => format!("an integer above {bound}"),
                BigIntValidator::GreaterThanOrEqualToBigInt(bound) => {
                    format!("an integer of at least {bound}")
                }
                BigIntValidator::LessThanBigInt(bound) => format!("an integer below {bound}"),
                BigIntValidator::LessThanOrEqualToBigInt(bound) => {
                    format!("an integer of at most {bound}")
                }
                BigIntValidator::BetweenBigInt(start, end) => {
                    format!("an integer from {start} to {end}")
                }
            },
            Validator::BigDecimalValidator(decimal) => match decimal {
                BigDecimalValidator::PositiveBigDecimal => "a positive decimal".to_string(),
                BigDecimalValidator::NegativeBigDecimal => "a negative decimal".to_string(),
                BigDecimalValidator::NonNegativeBigDecimal => "a decimal of at least 0".to_string(),
                BigDecimalValidator::NonPositiveBigDecimal => "a decimal of at most 0".to_string(),
                BigDecimalValidator::GreaterThanBigDecimal(bound) => {
                    format!("a decimal above {bound}")
                }
                BigDecimalValidator::GreaterThanOrEqualToBigDecimal(bound) => {
                    format!("a decimal of at least {bound}")
                }
                BigDecimalValidator::LessThanBigDecimal(bound) => {
                    format!("a decimal below {bound}")
                }
                BigDecimalValidator::LessThanOrEqualToBigDecimal(bound) => {
                    format!("a decimal of at most {bound}")
                }
                BigDecimalValidator::BetweenBigDecimal(start, end) => {
                    format!("a decimal from {start} to {end}")
                }
            },
            Validator::DurationValidator(duration) => match duration {
                DurationValidator::GreaterThanDuration(bound) => {
                    format!("a duration above {bound}")
                }
                DurationValidator::GreaterThanOrEqualToDuration(bound) => {
                    format!("a duration of at least {bound}")
                }
                DurationValidator::LessThanDuration(bound) => format!("a duration below {bound}"),
                DurationValidator::LessThanOrEqualToDuration(bound) => {
                    format!("a duration of at most {bound}")
                }
                DurationValidator::BetweenDuration(start, end) => {
                    format!("a duration from {start} to {end}")
                }
            },
        }
    }

    /// Whether a stored value satisfies this validator. Mockmake checks each
    /// candidate it generates. A validator has no opinion on a value outside
    /// its domain (a `NumberValidator` against a string), and an unparsable
    /// bound rejects every value.
    pub fn matches(&self, value: &MockValue) -> bool {
        if let Err(error) = self.check_bounds() {
            tracing::error!("{error}");
            return false;
        }
        match (self, value) {
            (Validator::StringValidator(validator), MockValue::Str(text)) => {
                validator.holds_for(text)
            }
            (Validator::NumberValidator(validator), MockValue::Num(number)) => {
                runtime::check_number(number, validator).is_ok()
            }
            (Validator::ArrayValidator(validator), MockValue::ArrayLen(count)) => match validator {
                ArrayValidator::MinItems(minimum) => count >= minimum,
                ArrayValidator::MaxItems(maximum) => count <= maximum,
                ArrayValidator::ItemsCount(exact) => count == exact,
            },
            (Validator::DateValidator(validator), MockValue::Date(date)) => {
                runtime::check_date(date, validator).is_ok()
            }
            (Validator::BigIntValidator(validator), MockValue::BigInt(text)) => {
                runtime::check_big_int(*text, validator).is_ok()
            }
            (Validator::BigDecimalValidator(validator), MockValue::BigDecimal(text)) => {
                runtime::check_decimal(*text, validator).is_ok()
            }
            (Validator::DurationValidator(validator), MockValue::DurationNanos(nanos)) => {
                runtime::check_duration(&runtime::Nanos(*nanos), validator).is_ok()
            }
            _ => true,
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
                quote! { ::evenframe::validator::NumberValidator::GreaterThan(::evenframe::prelude::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::GreaterThanOrEqualTo(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::GreaterThanOrEqualTo(::evenframe::prelude::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::LessThan(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::LessThan(::evenframe::prelude::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::LessThanOrEqualTo(v) => {
                let f = v.0;
                quote! { ::evenframe::validator::NumberValidator::LessThanOrEqualTo(::evenframe::prelude::ordered_float::OrderedFloat(#f)) }
            }
            NumberValidator::Between(start, end) => {
                let s = start.0;
                let e = end.0;
                quote! { ::evenframe::validator::NumberValidator::Between(::evenframe::prelude::ordered_float::OrderedFloat(#s), ::evenframe::prelude::ordered_float::OrderedFloat(#e)) }
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
                quote! { ::evenframe::validator::NumberValidator::MultipleOf(::evenframe::prelude::ordered_float::OrderedFloat(#f)) }
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

    // ==================== validation_tokens Tests ====================

    #[test]
    fn validation_tokens_call_the_runtime_family() {
        let place = quote! { value };
        let check = Validator::StringValidator(StringValidator::Email)
            .validation_tokens(&place, "email")
            .unwrap()
            .to_string();
        assert!(check.contains("check_string"));
        let transform = Validator::StringValidator(StringValidator::Lower)
            .validation_tokens(&place, "name")
            .unwrap()
            .to_string();
        assert!(transform.contains("transform_string"));
        let count = Validator::ArrayValidator(ArrayValidator::MinItems(3))
            .validation_tokens(&place, "tags")
            .unwrap()
            .to_string();
        assert!(count.contains("check_items"));
    }

    #[test]
    fn validation_tokens_reject_misplaced_parses_and_bad_bounds() {
        let place = quote! { value };
        assert!(
            Validator::StringValidator(StringValidator::IntegerParse)
                .validation_tokens(&place, "count")
                .is_err()
        );
        assert!(
            Validator::DateValidator(DateValidator::LessThanDate("soon".into()))
                .validation_tokens(&place, "due")
                .is_err()
        );
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
        let v =
            Validator::BigIntValidator(BigIntValidator::BetweenBigInt("0".into(), "100".into()));
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
        assert_eq!(
            super::parse_duration_to_nanos("1m"),
            Some(60 * 1_000_000_000)
        );
        assert_eq!(
            super::parse_duration_to_nanos("1h30m"),
            Some(90 * 60 * 1_000_000_000)
        );
        assert_eq!(super::parse_duration_to_nanos("500ms"), Some(500_000_000));
        assert_eq!(super::parse_duration_to_nanos("garbage"), None);
        assert_eq!(super::parse_duration_to_nanos(""), None);
    }
}
