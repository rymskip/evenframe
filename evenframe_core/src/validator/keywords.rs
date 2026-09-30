//! ArkType's string keywords, defined once for every consumer: the derive's
//! deserializer, mockmake, SurrealDB assertions and the TypeScript generators.
//!
//! Patterns use explicit ASCII classes and no lookaround or backreferences,
//! so one source means the same thing in JavaScript, the `regex` crate and
//! SurrealDB's `string::matches`. [`ISO_8601`] is the exception: it needs
//! both, so Rust runs it through `fancy-regex`. Keywords ArkType defines by
//! predicate are Rust functions here.

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, Utc};
use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// JavaScript's `WhiteSpace` and `LineTerminator` code points, which `\s`,
/// `\S` and `String.prototype.trim` use.
const JS_WHITESPACE: &str =
    r"\t\n\v\f\r \u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff";

pub const ALPHA: &str = r"^[A-Za-z]*$";
pub const ALPHANUMERIC: &str = r"^[0-9A-Za-z]*$";
pub const BASE64: &str = r"^(?:[0-9+/A-Za-z]{4})*(?:[0-9+/A-Za-z]{2}==|[0-9+/A-Za-z]{3}=)?$";
pub const BASE64_URL: &str =
    r"^(?:[0-9A-Za-z_-]{4})*(?:[0-9A-Za-z_-]{2}(?:==|%3D%3D)?|[0-9A-Za-z_-]{3}(?:=|%3D)?)?$";
pub const CAPITALIZED: &str = r"^[A-Z][^\n\r\u2028\u2029]*$";
pub const CREDIT_CARD: &str = r"^(?:4[0-9]{12}(?:[0-9]{3,6})?|5[1-5][0-9]{14}|(222[1-9]|22[3-9][0-9]|2[3-6][0-9]{2}|27[01][0-9]|2720)[0-9]{12}|6(?:011|5[0-9][0-9])[0-9]{12,15}|3[47][0-9]{13}|3(?:0[0-5]|[68][0-9])[0-9]{11}|(?:2131|1800|35[0-9]{3})[0-9]{11}|6[27][0-9]{14}|^(81[0-9]{14,17}))$";
pub const DIGITS: &str = r"^[0-9]*$";
pub const EMAIL: &str = r"^[0-9A-Za-z_%+.-]+@[0-9.A-Za-z-]+\.[A-Za-z]{2,}$";
pub const HEX: &str = r"^[0-9A-Fa-f]+$";
/// ArkType's well-formed integer: no leading zeros and no `-0`.
pub const INTEGER: &str = r"^(?:0|-?[1-9][0-9]*)$";
pub const IPV4: &str = r"^(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])$";
pub const IPV6: &str = concat!(
    r"^((?:[0-9a-fA-F]{1,4}:){7}(?:[0-9a-fA-F]{1,4}|:)|",
    r"(?:[0-9a-fA-F]{1,4}:){6}(?:(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])|:[0-9a-fA-F]{1,4}|:)|",
    r"(?:[0-9a-fA-F]{1,4}:){5}(?::(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])|(:[0-9a-fA-F]{1,4}){1,2}|:)|",
    r"(?:[0-9a-fA-F]{1,4}:){4}(?:(:[0-9a-fA-F]{1,4}){0,1}:(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])|(:[0-9a-fA-F]{1,4}){1,3}|:)|",
    r"(?:[0-9a-fA-F]{1,4}:){3}(?:(:[0-9a-fA-F]{1,4}){0,2}:(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])|(:[0-9a-fA-F]{1,4}){1,4}|:)|",
    r"(?:[0-9a-fA-F]{1,4}:){2}(?:(:[0-9a-fA-F]{1,4}){0,3}:(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])|(:[0-9a-fA-F]{1,4}){1,5}|:)|",
    r"(?:[0-9a-fA-F]{1,4}:){1}(?:(:[0-9a-fA-F]{1,4}){0,4}:(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])|(:[0-9a-fA-F]{1,4}){1,6}|:)|",
    r"(?::((?::[0-9a-fA-F]{1,4}){0,5}:(?:(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])[.]){3}(?:[0-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])|(?::[0-9a-fA-F]{1,4}){1,7}|:))",
    r")(%[0-9a-zA-Z.]{1,})?$"
);
pub const LOWER: &str = r"^[a-z]*$";
/// ArkType's numeric string: `.5` and trailing zeros are allowed, `-0`
/// forms are not.
pub const NUMERIC: &str = r"^(?:(?:0|[1-9][0-9]*)(?:\.[0-9]+)?|-(?:[1-9][0-9]*(?:\.[0-9]+)?|0\.[0-9]*[1-9][0-9]*)|\.[0-9]+)$";
pub const SEMVER: &str = r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-((?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$";
pub const UPPER: &str = r"^[A-Z]*$";
pub const UUID: &str = r"^(?:[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}|00000000-0000-0000-0000-000000000000|ffffffff-ffff-ffff-ffff-ffffffffffff)$";

/// ArkType's ISO 8601 matcher, verbatim. It needs backreferences and a
/// lookahead, so JavaScript uses it directly and Rust goes through
/// `fancy-regex`.
pub const ISO_8601: &str = r"^([+-]?\d{4}(?!\d{2}\b))((-?)((0[1-9]|1[0-2])(\3([12]\d|0[1-9]|3[01]))?|W([0-4]\d|5[0-3])(-?[1-7])?|(00[1-9]|0[1-9]\d|[12]\d{2}|3([0-5]\d|6[1-6])))(T((([01]\d|2[0-3])((:?)[0-5]\d)?|24:?00)([,.]\d+(?!:))?)?(\17[0-5]\d([,.]\d+)?)?([Zz]|([+-])([01]\d|2[0-3]):?([0-5]\d)?)?)?)?$";

/// The latest instant a JavaScript `Date` can hold, in milliseconds either
/// side of the epoch.
pub const MAX_EPOCH_MILLIS: i64 = 8_640_000_000_000_000;
/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// The UUID pattern for one version.
pub fn uuid_version(version: char) -> String {
    format!(
        r"^[0-9a-fA-F]{{8}}-[0-9a-fA-F]{{4}}-{version}[0-9a-fA-F]{{3}}-[89abAB][0-9a-fA-F]{{3}}-[0-9a-fA-F]{{12}}$"
    )
}

/// ArkType's trimmed string: no leading or trailing JavaScript whitespace.
pub fn trimmed_pattern() -> String {
    format!(r"^(?:[^{JS_WHITESPACE}](?:[^\n\r\u2028\u2029]*[^{JS_WHITESPACE}])?)?$")
}

fn compiled(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| panic!("keyword pattern {pattern:?}: {error}"))
}

macro_rules! keyword_regex {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| compiled(&$pattern));
    };
}

keyword_regex!(ALPHA_RE, ALPHA);
keyword_regex!(ALPHANUMERIC_RE, ALPHANUMERIC);
keyword_regex!(BASE64_RE, BASE64);
keyword_regex!(BASE64_URL_RE, BASE64_URL);
keyword_regex!(CAPITALIZED_RE, CAPITALIZED);
keyword_regex!(CREDIT_CARD_RE, CREDIT_CARD);
keyword_regex!(DIGITS_RE, DIGITS);
keyword_regex!(EMAIL_RE, EMAIL);
keyword_regex!(HEX_RE, HEX);
keyword_regex!(INTEGER_RE, INTEGER);
keyword_regex!(IPV4_RE, IPV4);
keyword_regex!(IPV6_RE, IPV6);
keyword_regex!(LOWER_RE, LOWER);
keyword_regex!(NUMERIC_RE, NUMERIC);
keyword_regex!(SEMVER_RE, SEMVER);
keyword_regex!(TRIMMED_RE, trimmed_pattern());
keyword_regex!(UPPER_RE, UPPER);
keyword_regex!(UUID_RE, UUID);

static UUID_VERSION_RE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    ('1'..='8')
        .map(|version| compiled(&uuid_version(version)))
        .collect()
});

static ISO_8601_RE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| {
    // JavaScript's `\d` and `\b` are ASCII-only. The one `\b` follows two
    // digits, where an ASCII boundary means no ASCII word character follows.
    let pattern = ISO_8601
        .replace(r"\d", "[0-9]")
        .replace(r"\b", "(?![A-Za-z0-9_])");
    fancy_regex::Regex::new(&pattern).unwrap_or_else(|error| panic!("ISO 8601 pattern: {error}"))
});

pub fn is_alpha(value: &str) -> bool {
    ALPHA_RE.is_match(value)
}

pub fn is_alphanumeric(value: &str) -> bool {
    ALPHANUMERIC_RE.is_match(value)
}

pub fn is_base64(value: &str) -> bool {
    BASE64_RE.is_match(value)
}

pub fn is_base64_url(value: &str) -> bool {
    BASE64_URL_RE.is_match(value)
}

pub fn is_capitalized(value: &str) -> bool {
    CAPITALIZED_RE.is_match(value)
}

/// A card number that matches a known issuer and passes the Luhn check.
pub fn is_credit_card(value: &str) -> bool {
    CREDIT_CARD_RE.is_match(value) && is_luhn_valid(value)
}

/// validator.js's `isLuhnNumber`, which ArkType uses: spaces and dashes are
/// ignored and every second digit from the right is doubled.
pub fn is_luhn_valid(value: &str) -> bool {
    let digits: Option<Vec<u32>> = value
        .chars()
        .filter(|character| *character != ' ' && *character != '-')
        .map(|character| character.to_digit(10))
        .collect();
    let Some(digits) = digits else {
        return false;
    };
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(position, digit)| {
            if position % 2 == 1 {
                let doubled = digit * 2;
                if doubled >= 10 {
                    doubled % 10 + 1
                } else {
                    doubled
                }
            } else {
                *digit
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

pub fn is_digits(value: &str) -> bool {
    DIGITS_RE.is_match(value)
}

pub fn is_email(value: &str) -> bool {
    EMAIL_RE.is_match(value)
}

pub fn is_hex(value: &str) -> bool {
    HEX_RE.is_match(value)
}

pub fn is_integer(value: &str) -> bool {
    INTEGER_RE.is_match(value)
}

pub fn is_ip(value: &str) -> bool {
    is_ipv4(value) || is_ipv6(value)
}

pub fn is_ipv4(value: &str) -> bool {
    IPV4_RE.is_match(value)
}

pub fn is_ipv6(value: &str) -> bool {
    IPV6_RE.is_match(value)
}

pub fn is_iso_8601(value: &str) -> bool {
    ISO_8601_RE.is_match(value).unwrap_or_else(|error| {
        tracing::error!("ISO 8601 match on {value:?} failed: {error}");
        false
    })
}

pub fn is_json(value: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(value).is_ok()
}

pub fn is_lower(value: &str) -> bool {
    LOWER_RE.is_match(value)
}

pub fn is_numeric(value: &str) -> bool {
    NUMERIC_RE.is_match(value)
}

/// A pattern the JavaScript `RegExp` constructor accepts, approximated by
/// what `fancy-regex` can compile.
pub fn is_regex(value: &str) -> bool {
    fancy_regex::Regex::new(value).is_ok()
}

pub fn is_semver(value: &str) -> bool {
    SEMVER_RE.is_match(value)
}

pub fn is_trimmed(value: &str) -> bool {
    TRIMMED_RE.is_match(value)
}

pub fn is_upper(value: &str) -> bool {
    UPPER_RE.is_match(value)
}

/// A URL the WHATWG parser accepts, like JavaScript's `URL.canParse`.
pub fn is_url(value: &str) -> bool {
    url::Url::parse(value).is_ok()
}

pub fn is_uuid(value: &str) -> bool {
    UUID_RE.is_match(value)
}

/// A UUID of `version`, from 1 to 8.
pub fn is_uuid_version(value: &str, version: u8) -> bool {
    version
        .checked_sub(1)
        .and_then(|index| UUID_VERSION_RE.get(usize::from(index)))
        .is_some_and(|pattern| pattern.is_match(value))
}

/// An integer string within the range of a JavaScript `Date`.
pub fn is_epoch(value: &str) -> bool {
    parse_epoch_millis(value).is_some()
}

/// A string JavaScript's `Date` constructor would parse. The ECMAScript date
/// time format and RFC 2822 are recognized; engines accept further
/// implementation-defined formats that this does not.
pub fn is_parsable_date(value: &str) -> bool {
    parse_date(value).is_some()
}

/// Unicode normal forms, as `String.prototype.normalize` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalForm {
    Nfc,
    Nfd,
    Nfkc,
    Nfkd,
}

impl NormalForm {
    pub fn name(self) -> &'static str {
        match self {
            NormalForm::Nfc => "NFC",
            NormalForm::Nfd => "NFD",
            NormalForm::Nfkc => "NFKC",
            NormalForm::Nfkd => "NFKD",
        }
    }
}

pub fn normalize(value: &str, form: NormalForm) -> String {
    match form {
        NormalForm::Nfc => value.nfc().collect(),
        NormalForm::Nfd => value.nfd().collect(),
        NormalForm::Nfkc => value.nfkc().collect(),
        NormalForm::Nfkd => value.nfkd().collect(),
    }
}

pub fn is_normalized(value: &str, form: NormalForm) -> bool {
    normalize(value, form) == value
}

/// JavaScript's `WhiteSpace` or `LineTerminator`: Unicode `White_Space`
/// without U+0085, plus U+FEFF.
fn is_js_whitespace(character: char) -> bool {
    (character.is_whitespace() && character != '\u{85}') || character == '\u{feff}'
}

/// JavaScript's `String.prototype.trim`.
pub fn trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

/// ArkType's `string.capitalize` morph: the first UTF-16 unit uppercased,
/// the rest unchanged.
pub fn capitalize(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) if first.len_utf16() == 1 => first.to_uppercase().chain(characters).collect(),
        _ => value.to_owned(),
    }
}

/// A string's length as JavaScript's `.length` counts it: UTF-16 code units.
pub fn js_length(value: &str) -> usize {
    value.encode_utf16().count()
}

/// ArkType's `string.integer.parse`: a well-formed integer within
/// `Number.MIN_SAFE_INTEGER..=Number.MAX_SAFE_INTEGER`.
pub fn parse_safe_integer(value: &str) -> Option<i64> {
    if !is_integer(value) {
        return None;
    }
    value
        .parse::<i64>()
        .ok()
        .filter(|parsed| parsed.abs() <= MAX_SAFE_INTEGER)
}

/// ArkType's `string.numeric.parse`.
pub fn parse_numeric(value: &str) -> Option<f64> {
    if is_numeric(value) {
        value.parse::<f64>().ok()
    } else {
        None
    }
}

/// ArkType's `string.date.epoch`: an integer string of milliseconds within
/// the range of a JavaScript `Date`.
pub fn parse_epoch_millis(value: &str) -> Option<i64> {
    parse_safe_integer(value).filter(|millis| millis.abs() <= MAX_EPOCH_MILLIS)
}

/// The instant `value` names, for the formats [`is_parsable_date`] accepts.
/// A date without a time is midnight UTC, as in JavaScript. A date-time
/// without an offset is also taken as UTC; JavaScript would use the local
/// time zone.
pub fn parse_date(value: &str) -> Option<DateTime<Utc>> {
    if let Some(date) = parse_date_only(value) {
        return Some(date.and_time(NaiveTime::MIN).and_utc());
    }
    if let Some(instant) = parse_date_time(value) {
        return Some(instant);
    }
    DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|instant| instant.with_timezone(&Utc))
}

/// The ECMAScript date time string format: a date form, `T`, `HH:mm` with
/// optional seconds and fraction, then `Z`, `+HH:mm`, `-HH:mm` or nothing.
fn parse_date_time(value: &str) -> Option<DateTime<Utc>> {
    let (date_part, time_part) = value.split_once('T')?;
    let date = parse_date_only(date_part)?;
    let (clock, offset) = split_offset(time_part)?;
    let time = ["%H:%M", "%H:%M:%S", "%H:%M:%S%.f"]
        .into_iter()
        .find_map(|format| NaiveTime::parse_from_str(clock, format).ok())?;
    date.and_time(time).and_utc().checked_sub_signed(offset)
}

/// Splits a trailing `Z` or `±HH:mm` offset from a time.
fn split_offset(time: &str) -> Option<(&str, TimeDelta)> {
    if let Some(clock) = time.strip_suffix('Z') {
        return Some((clock, TimeDelta::zero()));
    }
    let Some(sign_index) = time.len().checked_sub(6) else {
        return Some((time, TimeDelta::zero()));
    };
    let (clock, offset) = time.split_at_checked(sign_index)?;
    let sign = match offset.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return Some((time, TimeDelta::zero())),
    };
    let (hours, minutes) = offset.get(1..)?.split_once(':')?;
    let hours: i64 = fixed_width_number(hours, 2)?;
    let minutes: i64 = fixed_width_number(minutes, 2)?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some((clock, TimeDelta::minutes(sign * (hours * 60 + minutes))))
}

/// `part` as a number, when it is exactly `width` ASCII digits.
fn fixed_width_number<T: std::str::FromStr>(part: &str, width: usize) -> Option<T> {
    if part.len() == width && part.bytes().all(|byte| byte.is_ascii_digit()) {
        part.parse().ok()
    } else {
        None
    }
}

/// The ECMAScript date-only forms: `YYYY`, `YYYY-MM` and `YYYY-MM-DD`.
fn parse_date_only(value: &str) -> Option<NaiveDate> {
    let parts: Vec<&str> = value.split('-').collect();
    match parts.as_slice() {
        [year] => NaiveDate::from_ymd_opt(fixed_width_number(year, 4)?, 1, 1),
        [year, month] => NaiveDate::from_ymd_opt(
            fixed_width_number(year, 4)?,
            fixed_width_number(month, 2)?,
            1,
        ),
        [year, month, day] => NaiveDate::from_ymd_opt(
            fixed_width_number(year, 4)?,
            fixed_width_number(month, 2)?,
            fixed_width_number(day, 2)?,
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_and_numeric_follow_arktype_well_formedness() {
        for accepted in ["0", "7", "-7", "120"] {
            assert!(is_integer(accepted), "{accepted}");
        }
        for rejected in ["-0", "01", "+1", "1.0", ""] {
            assert!(!is_integer(rejected), "{rejected}");
        }
        for accepted in ["0", "-0.5", ".5", "0.10", "12.25"] {
            assert!(is_numeric(accepted), "{accepted}");
        }
        for rejected in ["-0", "-0.0", "-.5", "01", "1.", ""] {
            assert!(!is_numeric(rejected), "{rejected}");
        }
    }

    #[test]
    fn character_classes_are_ascii_like_javascript() {
        assert!(!is_digits("١٢٣"));
        assert!(!is_alpha("é"));
        assert!(is_alpha(""));
        assert!(is_email("first.last+tag@example.co"));
        assert!(!is_email("first@localhost"));
    }

    #[test]
    fn trimming_uses_javascript_whitespace() {
        assert_eq!(trim("\u{feff} value \u{3000}"), "value");
        assert!(is_trimmed("a b"));
        assert!(is_trimmed("a"));
        assert!(is_trimmed(""));
        assert!(!is_trimmed(" a"));
        assert!(!is_trimmed("a\u{feff}"));
    }

    #[test]
    fn capitalize_only_touches_the_first_unit() {
        assert_eq!(capitalize("hello World"), "Hello World");
        assert_eq!(capitalize("ßx"), "SSx");
        assert_eq!(capitalize(""), "");
        assert_eq!(capitalize("😀a"), "😀a");
    }

    #[test]
    fn credit_cards_need_an_issuer_and_the_luhn_check() {
        assert!(is_credit_card("4111111111111111"));
        assert!(!is_credit_card("4111111111111112"));
        assert!(!is_credit_card("1234567812345670"));
    }

    #[test]
    fn uuids_match_by_version() {
        let v4 = "9b2e8f5c-1d3a-4c6e-8f7a-2b4c6d8e0f1a";
        assert!(is_uuid(v4));
        assert!(is_uuid_version(v4, 4));
        assert!(!is_uuid_version(v4, 7));
        assert!(is_uuid("00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn iso_8601_uses_arktypes_matcher() {
        for accepted in [
            "2024-02-29",
            "2024-02-29T10:15:30Z",
            "2024-W05-3",
            "20240229",
        ] {
            assert!(is_iso_8601(accepted), "{accepted}");
        }
        for rejected in ["2024-13-01", "2024-02-29T25:00", "yesterday", "2024-0229"] {
            assert!(!is_iso_8601(rejected), "{rejected}");
        }
    }

    #[test]
    fn dates_and_epochs_parse_like_javascript() {
        assert_eq!(
            parse_date("2024-02-29").map(|instant| instant.to_rfc3339()),
            Some("2024-02-29T00:00:00+00:00".to_string())
        );
        assert!(parse_date("2024-02-30").is_none());
        assert_eq!(
            parse_date("2024-02-29T10:15+02:00").map(|instant| instant.to_rfc3339()),
            Some("2024-02-29T08:15:00+00:00".to_string())
        );
        assert!(parse_date("Thu, 29 Feb 2024 10:15:00 +0000").is_some());
        assert!(is_epoch("8640000000000000"));
        assert!(!is_epoch("8640000000000001"));
        assert_eq!(parse_safe_integer("9007199254740992"), None);
    }

    #[test]
    fn javascript_length_counts_utf16_units() {
        assert_eq!(js_length("é"), 1);
        assert_eq!(js_length("😀"), 2);
    }

    #[test]
    fn patterns_compile_as_rust_regexes() {
        for pattern in [
            ALPHA,
            ALPHANUMERIC,
            BASE64,
            BASE64_URL,
            CAPITALIZED,
            CREDIT_CARD,
            DIGITS,
            EMAIL,
            HEX,
            INTEGER,
            IPV4,
            IPV6,
            LOWER,
            NUMERIC,
            SEMVER,
            UPPER,
            UUID,
        ] {
            assert!(Regex::new(pattern).is_ok(), "{pattern}");
        }
        assert!(Regex::new(&trimmed_pattern()).is_ok());
        assert!(is_ipv6("2001:db8::1"));
        assert!(is_ipv4("192.168.0.1"));
        assert!(!is_ipv4("256.1.1.1"));
    }
}
