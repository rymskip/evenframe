//! What each [`StringValidator`] does to a value. Variants named after
//! ArkType keywords follow ArkType; the Effect-named filters (`Trimmed`,
//! `Lowercased`, `Capitalized`, ...) follow Effect.

use super::StringValidator;
use super::bounds;
use super::keywords::{self, NormalForm};
use super::text_pattern::{Anchoring, TextPattern};
use crate::schemasync::mockmake::format::Format;
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

impl StringValidator {
    /// Whether `value` is an input this validator accepts.
    pub fn accepts(&self, value: &str) -> bool {
        match self {
            StringValidator::String => true,
            StringValidator::Alpha => keywords::is_alpha(value),
            StringValidator::Alphanumeric => keywords::is_alphanumeric(value),
            StringValidator::Base64 => keywords::is_base64(value),
            StringValidator::Base64Url => keywords::is_base64_url(value),
            StringValidator::CapitalizePreformatted => keywords::is_capitalized(value),
            StringValidator::CreditCard => keywords::is_credit_card(value),
            StringValidator::Date => keywords::is_parsable_date(value),
            StringValidator::DateEpoch => keywords::is_epoch(value),
            StringValidator::DateIso => keywords::is_iso_8601(value),
            StringValidator::Digits => keywords::is_digits(value),
            StringValidator::Email => keywords::is_email(value),
            StringValidator::Hex => keywords::is_hex(value),
            StringValidator::Integer => keywords::is_integer(value),
            StringValidator::Ip => keywords::is_ip(value),
            StringValidator::IpV4 => keywords::is_ipv4(value),
            StringValidator::IpV6 => keywords::is_ipv6(value),
            StringValidator::Json => keywords::is_json(value),
            StringValidator::LowerPreformatted => keywords::is_lower(value),
            StringValidator::NormalizeNfcPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfc)
            }
            StringValidator::NormalizeNfdPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfd)
            }
            StringValidator::NormalizeNfkcPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfkc)
            }
            StringValidator::NormalizeNfkdPreformatted => {
                keywords::is_normalized(value, NormalForm::Nfkd)
            }
            StringValidator::Numeric => keywords::is_numeric(value),
            StringValidator::Regex => keywords::is_regex(value),
            StringValidator::Semver => keywords::is_semver(value),
            StringValidator::TrimPreformatted => keywords::is_trimmed(value),
            StringValidator::UpperPreformatted => keywords::is_upper(value),
            StringValidator::Url => keywords::is_url(value),
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
            StringValidator::StartsWith(pattern) => text_holds(pattern, Anchoring::Start, value),
            StringValidator::EndsWith(pattern) => text_holds(pattern, Anchoring::End, value),
            StringValidator::Includes(pattern) => text_holds(pattern, Anchoring::Anywhere, value),
            StringValidator::Trimmed => keywords::trim(value) == value,
            StringValidator::Lowercased => value.to_lowercase() == value,
            StringValidator::Uppercased => value.to_uppercase() == value,
            StringValidator::Capitalized => first_unit_is(value, char::to_uppercase),
            StringValidator::Uncapitalized => first_unit_is(value, char::to_lowercase),
        }
    }

    /// Why `value` fails this validator, worded as what was expected.
    pub fn expectation(&self) -> String {
        match self {
            StringValidator::Literal(literal) => format!("exactly \"{literal}\""),
            StringValidator::RegexLiteral(format) => format.description(),
            StringValidator::Length(bound) => format!("exactly {bound} characters"),
            StringValidator::MinLength(length) => format!("at least {length} characters"),
            StringValidator::MaxLength(length) => format!("at most {length} characters"),
            StringValidator::StartsWith(pattern) => {
                format!("a value starting with {}", pattern.describe())
            }
            StringValidator::EndsWith(pattern) => {
                format!("a value ending with {}", pattern.describe())
            }
            StringValidator::Includes(pattern) => {
                format!("a value containing {}", pattern.describe())
            }
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
            StringValidator::CapitalizePreformatted => "string.capitalize.preformatted",
            StringValidator::CreditCard => "string.creditCard",
            StringValidator::Date => "string.date",
            StringValidator::DateEpoch => "string.date.epoch",
            StringValidator::DateIso => "string.date.iso",
            StringValidator::Digits => "string.digits",
            StringValidator::Email => "string.email",
            StringValidator::Hex => "string.hex",
            StringValidator::Integer => "string.integer",
            StringValidator::Ip => "string.ip",
            StringValidator::IpV4 => "string.ip.v4",
            StringValidator::IpV6 => "string.ip.v6",
            StringValidator::Json => "string.json",
            StringValidator::LowerPreformatted => "string.lower.preformatted",
            StringValidator::NormalizeNfcPreformatted => "string.normalize.NFC.preformatted",
            StringValidator::NormalizeNfdPreformatted => "string.normalize.NFD.preformatted",
            StringValidator::NormalizeNfkcPreformatted => "string.normalize.NFKC.preformatted",
            StringValidator::NormalizeNfkdPreformatted => "string.normalize.NFKD.preformatted",
            StringValidator::Numeric => "string.numeric",
            StringValidator::Regex => "string.regex",
            StringValidator::Semver => "string.semver",
            StringValidator::TrimPreformatted => "string.trim.preformatted",
            StringValidator::UpperPreformatted => "string.upper.preformatted",
            StringValidator::Url => "string.url",
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
            StringValidator::String => "a string",
            StringValidator::Alpha => "only letters",
            StringValidator::Alphanumeric => "only letters and digits 0-9",
            StringValidator::Base64 => "base64-encoded",
            StringValidator::Base64Url => "base64url-encoded",
            StringValidator::CapitalizePreformatted => "capitalized",
            StringValidator::CreditCard => "a credit card number",
            StringValidator::Date => "a parsable date",
            StringValidator::DateEpoch => "an integer string representing a safe Unix timestamp",
            StringValidator::DateIso => "an ISO 8601 (YYYY-MM-DDTHH:mm:ss.sssZ) date",
            StringValidator::Digits => "only digits 0-9",
            StringValidator::Email => "an email address",
            StringValidator::Hex => "hex characters only",
            StringValidator::Integer => "a well-formed integer string",
            StringValidator::Ip => "an IP address",
            StringValidator::IpV4 => "an IPv4 address",
            StringValidator::IpV6 => "an IPv6 address",
            StringValidator::Json => "a JSON string",
            StringValidator::LowerPreformatted => "only lowercase letters",
            StringValidator::NormalizeNfcPreformatted => "NFC-normalized unicode",
            StringValidator::NormalizeNfdPreformatted => "NFD-normalized unicode",
            StringValidator::NormalizeNfkcPreformatted => "NFKC-normalized unicode",
            StringValidator::NormalizeNfkdPreformatted => "NFKD-normalized unicode",
            StringValidator::Numeric => "a well-formed numeric string",
            StringValidator::Regex => "a regex pattern",
            StringValidator::Semver => "a semantic version (see https://semver.org/)",
            StringValidator::TrimPreformatted | StringValidator::Trimmed => "trimmed",
            StringValidator::UpperPreformatted => "only uppercase letters",
            StringValidator::Url => "a URL string",
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

/// Whether `value` holds `pattern` where `anchoring` looks for it.
fn text_holds(pattern: &TextPattern, anchoring: Anchoring, value: &str) -> bool {
    match pattern {
        TextPattern::Text(text) => match anchoring {
            Anchoring::Start => value.starts_with(text.as_str()),
            Anchoring::End => value.ends_with(text.as_str()),
            Anchoring::Anywhere => value.contains(text.as_str()),
        },
        TextPattern::Format(Format::Custom(custom)) if custom.flags().is_some() => {
            javascript_is_match(
                &anchoring.anchor(custom.as_str()),
                custom.flags().unwrap_or_default(),
                value,
            )
        }
        TextPattern::Format(format) => {
            text_regex(format, anchoring).is_some_and(|regex| regex.is_match(value))
        }
    }
}

/// Each format's body as a text argument anchors it, `None` for one that
/// does not compile.
type TextRegexes = HashMap<(Format, Anchoring), Option<Arc<Regex>>>;

static TEXT_REGEXES: LazyLock<Mutex<TextRegexes>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// `format`'s body anchored as `anchoring` says, compiled once per process. A
/// pattern that does not compile is logged and matches nothing.
fn text_regex(format: &Format, anchoring: Anchoring) -> Option<Arc<Regex>> {
    let mut cache = TEXT_REGEXES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache
        .entry((format.clone(), anchoring))
        .or_insert_with(|| {
            let source = anchoring.anchor(&format.body());
            match Regex::new(&source) {
                Ok(regex) => Some(Arc::new(regex)),
                Err(error) => {
                    tracing::error!("the pattern /{source}/ does not compile: {error}");
                    None
                }
            }
        })
        .clone()
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
    use super::StringValidator;

    #[test]
    fn preformatted_checks_reject_what_their_morph_would_rewrite() {
        assert!(StringValidator::LowerPreformatted.accepts("hello"));
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
