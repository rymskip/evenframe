//! What each [`StringValidator`] does to a value. Variants named after
//! ArkType keywords follow ArkType; the Effect-named filters (`Trimmed`,
//! `Lowercased`, `Capitalized`, ...) follow Effect.

use super::StringValidator;
use super::bounds;
use super::keywords::{self, NormalForm};
use crate::schemasync::mockmake::format::Format;
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// A morph that rewrites a string and keeps it a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringTransform {
    Lower,
    Upper,
    Trim,
    Capitalize,
    Normalize(NormalForm),
}

impl StringTransform {
    pub fn apply(self, value: &str) -> String {
        match self {
            StringTransform::Lower => value.to_lowercase(),
            StringTransform::Upper => value.to_uppercase(),
            StringTransform::Trim => keywords::trim(value).to_owned(),
            StringTransform::Capitalize => keywords::capitalize(value),
            StringTransform::Normalize(form) => keywords::normalize(value, form),
        }
    }
}

/// A morph that parses a string into the field's own type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringParse {
    Integer,
    Numeric,
    Date,
    DateIso,
    DateEpoch,
    Json,
    Url,
}

/// The role a string validator plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringRule {
    /// Accepts or rejects the value.
    Check,
    /// Rewrites the value.
    Transform(StringTransform),
    /// Reads a string and parses it into the field's type.
    Parse(StringParse),
    /// Carries text with no per-value meaning.
    Carrier,
}

impl StringValidator {
    pub fn rule(&self) -> StringRule {
        match self {
            StringValidator::Capitalize => StringRule::Transform(StringTransform::Capitalize),
            StringValidator::Lower => StringRule::Transform(StringTransform::Lower),
            StringValidator::Upper => StringRule::Transform(StringTransform::Upper),
            StringValidator::Trim => StringRule::Transform(StringTransform::Trim),
            StringValidator::Normalize | StringValidator::NormalizeNFC => {
                StringRule::Transform(StringTransform::Normalize(NormalForm::Nfc))
            }
            StringValidator::NormalizeNFD => {
                StringRule::Transform(StringTransform::Normalize(NormalForm::Nfd))
            }
            StringValidator::NormalizeNFKC => {
                StringRule::Transform(StringTransform::Normalize(NormalForm::Nfkc))
            }
            StringValidator::NormalizeNFKD => {
                StringRule::Transform(StringTransform::Normalize(NormalForm::Nfkd))
            }
            StringValidator::IntegerParse => StringRule::Parse(StringParse::Integer),
            StringValidator::NumericParse => StringRule::Parse(StringParse::Numeric),
            StringValidator::DateParse => StringRule::Parse(StringParse::Date),
            StringValidator::DateIsoParse => StringRule::Parse(StringParse::DateIso),
            StringValidator::DateEpochParse => StringRule::Parse(StringParse::DateEpoch),
            StringValidator::JsonParse => StringRule::Parse(StringParse::Json),
            StringValidator::UrlParse => StringRule::Parse(StringParse::Url),
            StringValidator::StringEmbedded(_) => StringRule::Carrier,
            _ => StringRule::Check,
        }
    }

    /// Whether `value` is an input this validator accepts. A transform
    /// accepts any string; a parse accepts the strings it can parse.
    pub fn accepts(&self, value: &str) -> bool {
        match self {
            StringValidator::String
            | StringValidator::StringEmbedded(_)
            | StringValidator::Capitalize
            | StringValidator::Lower
            | StringValidator::Upper
            | StringValidator::Trim
            | StringValidator::Normalize
            | StringValidator::NormalizeNFC
            | StringValidator::NormalizeNFD
            | StringValidator::NormalizeNFKC
            | StringValidator::NormalizeNFKD => true,
            StringValidator::Alpha => keywords::is_alpha(value),
            StringValidator::Alphanumeric => keywords::is_alphanumeric(value),
            StringValidator::Base64 => keywords::is_base64(value),
            StringValidator::Base64Url => keywords::is_base64_url(value),
            StringValidator::CapitalizePreformatted => keywords::is_capitalized(value),
            StringValidator::CreditCard => keywords::is_credit_card(value),
            StringValidator::Date | StringValidator::DateParse => keywords::is_parsable_date(value),
            StringValidator::DateEpoch | StringValidator::DateEpochParse => {
                keywords::is_epoch(value)
            }
            StringValidator::DateIso => keywords::is_iso_8601(value),
            StringValidator::DateIsoParse => {
                keywords::is_iso_8601(value) && keywords::is_parsable_date(value)
            }
            StringValidator::Digits => keywords::is_digits(value),
            StringValidator::Email => keywords::is_email(value),
            StringValidator::Hex => keywords::is_hex(value),
            StringValidator::Integer => keywords::is_integer(value),
            StringValidator::IntegerParse => keywords::parse_safe_integer(value).is_some(),
            StringValidator::Ip => keywords::is_ip(value),
            StringValidator::IpV4 => keywords::is_ipv4(value),
            StringValidator::IpV6 => keywords::is_ipv6(value),
            StringValidator::Json => keywords::is_json(value),
            StringValidator::JsonParse => !value.is_empty() && keywords::is_json(value),
            StringValidator::LowerPreformatted => keywords::is_lower(value),
            StringValidator::NormalizeNFCPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfc)
            }
            StringValidator::NormalizeNFDPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfd)
            }
            StringValidator::NormalizeNFKCPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfkc)
            }
            StringValidator::NormalizeNFKDPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfkd)
            }
            StringValidator::Numeric => keywords::is_numeric(value),
            StringValidator::NumericParse => keywords::parse_numeric(value).is_some(),
            StringValidator::Regex => keywords::is_regex(value),
            StringValidator::Semver => keywords::is_semver(value),
            StringValidator::TrimPreformatted => keywords::is_trimmed(value),
            StringValidator::UpperPreformatted => keywords::is_upper(value),
            StringValidator::Url | StringValidator::UrlParse => keywords::is_url(value),
            StringValidator::Uuid => keywords::is_uuid(value),
            StringValidator::UuidV1 => keywords::is_uuid_version(value, 1),
            StringValidator::UuidV2 => keywords::is_uuid_version(value, 2),
            StringValidator::UuidV3 => keywords::is_uuid_version(value, 3),
            StringValidator::UuidV4 => keywords::is_uuid_version(value, 4),
            StringValidator::UuidV5 => keywords::is_uuid_version(value, 5),
            StringValidator::UuidV6 => keywords::is_uuid_version(value, 6),
            StringValidator::UuidV7 => keywords::is_uuid_version(value, 7),
            StringValidator::UuidV8 => keywords::is_uuid_version(value, 8),
            StringValidator::Literal(literal) => value == literal,
            StringValidator::RegexLiteral(Format::Custom(custom)) if custom.flags().is_some() => {
                javascript_is_match(custom.as_str(), custom.flags().unwrap_or_default(), value)
            }
            StringValidator::RegexLiteral(format) => format_regex(format).is_match(value),
            StringValidator::Length(bound) => match bounds::length(bound) {
                Ok(length) => keywords::js_length(value) == length,
                Err(error) => {
                    tracing::error!("{error}");
                    false
                }
            },
            StringValidator::MinLength(length) => keywords::js_length(value) >= *length,
            StringValidator::MaxLength(length) => keywords::js_length(value) <= *length,
            StringValidator::NonEmpty => !value.is_empty(),
            StringValidator::StartsWith(prefix) => value.starts_with(prefix.as_str()),
            StringValidator::EndsWith(suffix) => value.ends_with(suffix.as_str()),
            StringValidator::Includes(substring) => value.contains(substring.as_str()),
            StringValidator::Trimmed => keywords::trim(value) == value,
            StringValidator::Lowercased => value.to_lowercase() == value,
            StringValidator::Uppercased => value.to_uppercase() == value,
            StringValidator::Capitalized => first_unit_is(value, char::to_uppercase),
            StringValidator::Uncapitalized => first_unit_is(value, char::to_lowercase),
        }
    }

    /// Whether a stored value satisfies this validator. A stored value has
    /// already been transformed, so it is its own transform.
    pub fn holds_for(&self, value: &str) -> bool {
        match self.rule() {
            StringRule::Transform(transform) => transform.apply(value) == value,
            StringRule::Check | StringRule::Parse(_) | StringRule::Carrier => self.accepts(value),
        }
    }

    /// Why `value` fails this validator, worded as what was expected.
    pub fn expectation(&self) -> String {
        match self {
            StringValidator::Literal(literal) => format!("exactly \"{literal}\""),
            StringValidator::RegexLiteral(format) => format!("a value in the {format:?} format"),
            StringValidator::Length(bound) => format!("exactly {bound} characters"),
            StringValidator::MinLength(length) => format!("at least {length} characters"),
            StringValidator::MaxLength(length) => format!("at most {length} characters"),
            StringValidator::StartsWith(prefix) => format!("a value starting with \"{prefix}\""),
            StringValidator::EndsWith(suffix) => format!("a value ending with \"{suffix}\""),
            StringValidator::Includes(substring) => format!("a value containing \"{substring}\""),
            other => other.description().to_owned(),
        }
    }

    /// The ArkType keyword this validator is named after, when it is one.
    pub fn arktype_keyword(&self) -> Option<&'static str> {
        Some(match self {
            StringValidator::Alpha => "string.alpha",
            StringValidator::Alphanumeric => "string.alphanumeric",
            StringValidator::Base64 => "string.base64",
            StringValidator::Base64Url => "string.base64.url",
            StringValidator::Capitalize => "string.capitalize",
            StringValidator::CapitalizePreformatted => "string.capitalize.preformatted",
            StringValidator::CreditCard => "string.creditCard",
            StringValidator::Date => "string.date",
            StringValidator::DateEpoch => "string.date.epoch",
            StringValidator::DateEpochParse => "string.date.epoch.parse",
            StringValidator::DateIso => "string.date.iso",
            StringValidator::DateIsoParse => "string.date.iso.parse",
            StringValidator::DateParse => "string.date.parse",
            StringValidator::Digits => "string.digits",
            StringValidator::Email => "string.email",
            StringValidator::Hex => "string.hex",
            StringValidator::Integer => "string.integer",
            StringValidator::IntegerParse => "string.integer.parse",
            StringValidator::Ip => "string.ip",
            StringValidator::IpV4 => "string.ip.v4",
            StringValidator::IpV6 => "string.ip.v6",
            StringValidator::Json => "string.json",
            StringValidator::JsonParse => "string.json.parse",
            StringValidator::Lower => "string.lower",
            StringValidator::LowerPreformatted => "string.lower.preformatted",
            StringValidator::Normalize => "string.normalize",
            StringValidator::NormalizeNFC => "string.normalize.NFC",
            StringValidator::NormalizeNFCPreformatted => "string.normalize.NFC.preformatted",
            StringValidator::NormalizeNFD => "string.normalize.NFD",
            StringValidator::NormalizeNFDPreformatted => "string.normalize.NFD.preformatted",
            StringValidator::NormalizeNFKC => "string.normalize.NFKC",
            StringValidator::NormalizeNFKCPreformatted => "string.normalize.NFKC.preformatted",
            StringValidator::NormalizeNFKD => "string.normalize.NFKD",
            StringValidator::NormalizeNFKDPreformatted => "string.normalize.NFKD.preformatted",
            StringValidator::Numeric => "string.numeric",
            StringValidator::NumericParse => "string.numeric.parse",
            StringValidator::Regex => "string.regex",
            StringValidator::Semver => "string.semver",
            StringValidator::Trim => "string.trim",
            StringValidator::TrimPreformatted => "string.trim.preformatted",
            StringValidator::Upper => "string.upper",
            StringValidator::UpperPreformatted => "string.upper.preformatted",
            StringValidator::Url => "string.url",
            StringValidator::UrlParse => "string.url.parse",
            StringValidator::Uuid => "string.uuid",
            StringValidator::UuidV1 => "string.uuid.v1",
            StringValidator::UuidV2 => "string.uuid.v2",
            StringValidator::UuidV3 => "string.uuid.v3",
            StringValidator::UuidV4 => "string.uuid.v4",
            StringValidator::UuidV5 => "string.uuid.v5",
            StringValidator::UuidV6 => "string.uuid.v6",
            StringValidator::UuidV7 => "string.uuid.v7",
            StringValidator::UuidV8 => "string.uuid.v8",
            _ => return None,
        })
    }

    /// ArkType's description of the keyword, or Effect's for its filters.
    pub fn description(&self) -> &'static str {
        match self {
            StringValidator::String | StringValidator::StringEmbedded(_) => "a string",
            StringValidator::Alpha => "only letters",
            StringValidator::Alphanumeric => "only letters and digits 0-9",
            StringValidator::Base64 => "base64-encoded",
            StringValidator::Base64Url => "base64url-encoded",
            StringValidator::Capitalize | StringValidator::CapitalizePreformatted => "capitalized",
            StringValidator::CreditCard => "a credit card number",
            StringValidator::Date | StringValidator::DateParse => "a parsable date",
            StringValidator::DateEpoch | StringValidator::DateEpochParse => {
                "an integer string representing a safe Unix timestamp"
            }
            StringValidator::DateIso | StringValidator::DateIsoParse => {
                "an ISO 8601 (YYYY-MM-DDTHH:mm:ss.sssZ) date"
            }
            StringValidator::Digits => "only digits 0-9",
            StringValidator::Email => "an email address",
            StringValidator::Hex => "hex characters only",
            StringValidator::Integer => "a well-formed integer string",
            StringValidator::IntegerParse => {
                "an integer in the range Number.MIN_SAFE_INTEGER to Number.MAX_SAFE_INTEGER"
            }
            StringValidator::Ip => "an IP address",
            StringValidator::IpV4 => "an IPv4 address",
            StringValidator::IpV6 => "an IPv6 address",
            StringValidator::Json | StringValidator::JsonParse => "a JSON string",
            StringValidator::Lower | StringValidator::LowerPreformatted => "only lowercase letters",
            StringValidator::Normalize
            | StringValidator::NormalizeNFC
            | StringValidator::NormalizeNFCPreformatted => "NFC-normalized unicode",
            StringValidator::NormalizeNFD | StringValidator::NormalizeNFDPreformatted => {
                "NFD-normalized unicode"
            }
            StringValidator::NormalizeNFKC | StringValidator::NormalizeNFKCPreformatted => {
                "NFKC-normalized unicode"
            }
            StringValidator::NormalizeNFKD | StringValidator::NormalizeNFKDPreformatted => {
                "NFKD-normalized unicode"
            }
            StringValidator::Numeric | StringValidator::NumericParse => {
                "a well-formed numeric string"
            }
            StringValidator::Regex => "a regex pattern",
            StringValidator::Semver => "a semantic version (see https://semver.org/)",
            StringValidator::Trim
            | StringValidator::TrimPreformatted
            | StringValidator::Trimmed => "trimmed",
            StringValidator::Upper | StringValidator::UpperPreformatted => "only uppercase letters",
            StringValidator::Url | StringValidator::UrlParse => "a URL string",
            StringValidator::Uuid => "a UUID",
            StringValidator::UuidV1 => "a UUIDv1",
            StringValidator::UuidV2 => "a UUIDv2",
            StringValidator::UuidV3 => "a UUIDv3",
            StringValidator::UuidV4 => "a UUIDv4",
            StringValidator::UuidV5 => "a UUIDv5",
            StringValidator::UuidV6 => "a UUIDv6",
            StringValidator::UuidV7 => "a UUIDv7",
            StringValidator::UuidV8 => "a UUIDv8",
            StringValidator::Literal(_) => "an exact value",
            StringValidator::RegexLiteral(_) => "a formatted value",
            StringValidator::Length(_) => "an exact length",
            StringValidator::MinLength(_) => "a minimum length",
            StringValidator::MaxLength(_) => "a maximum length",
            StringValidator::NonEmpty => "a non-empty string",
            StringValidator::StartsWith(_) => "a prefix",
            StringValidator::EndsWith(_) => "a suffix",
            StringValidator::Includes(_) => "a substring",
            StringValidator::Lowercased => "lowercase",
            StringValidator::Uppercased => "uppercase",
            StringValidator::Capitalized => "capitalized",
            StringValidator::Uncapitalized => "uncapitalized",
        }
    }
}

/// Effect's `capitalized` and `uncapitalized`: the first UTF-16 unit is
/// unchanged by the case mapping. An empty string passes.
fn first_unit_is<I: Iterator<Item = char>>(value: &str, map: impl Fn(char) -> I) -> bool {
    match value.chars().next() {
        Some(first) if first.len_utf16() == 1 => map(first).eq(std::iter::once(first)),
        _ => true,
    }
}

static FORMAT_REGEXES: LazyLock<Mutex<HashMap<Format, Arc<Regex>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Each typesync-only pattern by its source and flags, `None` for one that
/// does not compile.
type JavaScriptRegexes = HashMap<(String, String), Option<Arc<regress::Regex>>>;

static JAVASCRIPT_REGEXES: LazyLock<Mutex<JavaScriptRegexes>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether `value` matches a typesync-only pattern as JavaScript's `test`
/// would, compiled once per process. The pattern was checked when its
/// attribute was parsed, so one that does not compile is logged and matches
/// nothing.
fn javascript_is_match(source: &str, flags: &str, value: &str) -> bool {
    let regex = {
        let mut cache = JAVASCRIPT_REGEXES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .entry((source.to_owned(), flags.to_owned()))
            .or_insert_with(|| match regress::Regex::with_flags(source, flags) {
                Ok(regex) => Some(Arc::new(regex)),
                Err(error) => {
                    tracing::error!(
                        "the JavaScript pattern /{source}/{flags} does not compile: {error}"
                    );
                    None
                }
            })
            .clone()
    };
    regex.is_some_and(|regex| regex.find(value).is_some())
}

/// The compiled regex for `format`, compiled once per process.
pub fn format_regex(format: &Format) -> Arc<Regex> {
    let mut cache = FORMAT_REGEXES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(
        cache
            .entry(format.clone())
            .or_insert_with(|| Arc::new(format.regex())),
    )
}

#[cfg(test)]
mod tests {
    use super::{StringParse, StringRule, StringTransform, StringValidator};

    #[test]
    fn morphs_are_classified() {
        assert_eq!(
            StringValidator::Lower.rule(),
            StringRule::Transform(StringTransform::Lower)
        );
        assert_eq!(
            StringValidator::IntegerParse.rule(),
            StringRule::Parse(StringParse::Integer)
        );
        assert_eq!(StringValidator::LowerPreformatted.rule(), StringRule::Check);
        assert_eq!(StringValidator::Email.rule(), StringRule::Check);
    }

    #[test]
    fn stored_values_are_their_own_transform() {
        assert!(StringValidator::Lower.holds_for("hello world"));
        assert!(!StringValidator::Lower.holds_for("Hello"));
        assert!(StringValidator::Lower.accepts("Hello"));
        assert!(!StringValidator::LowerPreformatted.accepts("hello world"));
    }

    #[test]
    fn effect_filters_follow_effect() {
        assert!(StringValidator::Capitalized.accepts(""));
        assert!(StringValidator::Capitalized.accepts("Hello world"));
        assert!(!StringValidator::Capitalized.accepts("hello"));
        assert!(StringValidator::Lowercased.accepts("hello world 1"));
        assert!(StringValidator::MinLength(1).accepts("😀"));
        assert!(!StringValidator::MaxLength(1).accepts("😀"));
    }
}
