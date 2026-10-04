//! String checks as JavaScript, shared by the TypeScript generators. Each is
//! the ArkType or Effect definition the validator is named after, over the
//! keyword patterns in [`crate::validator::keywords`].

use crate::error::{EvenframeError, Result};
use crate::validator::keywords::{self, NormalForm};
use crate::validator::{BigDecimalValidator, DurationValidator, StringValidator};
use crate::validator::{bounds, runtime};

/// How a string check is written in JavaScript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsCheck {
    /// A regex the value must match, as its source.
    Pattern(String),
    /// A boolean expression over `value`.
    Predicate(String),
    /// A length constraint both libraries express natively.
    Length(LengthCheck),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthCheck {
    Exactly(usize),
    AtLeast(usize),
    AtMost(usize),
}

/// Exactly one Unicode scalar value, which is what a Rust `char` holds.
/// JavaScript counts UTF-16 units, so a surrogate pair is one character, and
/// ArkType's regex syntax takes no flags, so the pattern spells that out.
pub const ONE_CHARACTER: &str = r"^(?:[\uD800-\uDBFF][\uDC00-\uDFFF]|[^\uD800-\uDFFF])$";

/// `text` as a JavaScript string literal.
pub fn string_literal(text: &str) -> Result<String> {
    serde_json::to_string(text).map_err(|error| {
        EvenframeError::config(format!(
            "cannot encode {text:?} as a string literal: {error}"
        ))
    })
}

/// `name` as an object key: bare when it is an identifier, quoted otherwise.
pub fn object_key(name: &str) -> Result<String> {
    let mut characters = name.chars();
    let is_identifier = characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_' || first == '$')
        && characters.all(|rest| rest.is_ascii_alphanumeric() || rest == '_' || rest == '$');
    if is_identifier {
        Ok(name.to_owned())
    } else {
        string_literal(name)
    }
}

/// `text` as a JavaScript template literal.
pub fn template_literal(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace("${", "\\${");
    format!("`{escaped}`")
}

fn pattern(source: &str) -> JsCheck {
    JsCheck::Pattern(source.to_string())
}

fn test(source: &str) -> Result<String> {
    Ok(format!(
        "new RegExp({}).test(value)",
        string_literal(source)?
    ))
}

/// validator.js's `isLuhnNumber`, which ArkType's credit card keyword uses.
const LUHN: &str = "((digits) => { let sum = 0; let double = false; \
for (let index = digits.length - 1; index >= 0; index--) { let digit = Number.parseInt(digits.charAt(index), 10); \
if (double) { digit *= 2; sum += digit >= 10 ? (digit % 10) + 1 : digit; } else { sum += digit; } double = !double; } \
return sum % 10 === 0; })(value.replace(/[ -]+/g, \"\"))";

fn normalized(form: NormalForm) -> JsCheck {
    JsCheck::Predicate(format!("value.normalize(\"{}\") === value", form.name()))
}

/// The JavaScript form of a string check, or `None` for a validator that is
/// not a check (a transform, a parse or a carrier).
pub fn string_check(validator: &StringValidator) -> Result<Option<JsCheck>> {
    let check = match validator {
        StringValidator::Alpha => pattern(keywords::ALPHA),
        StringValidator::Alphanumeric => pattern(keywords::ALPHANUMERIC),
        StringValidator::Base64 => pattern(keywords::BASE64),
        StringValidator::Base64Url => pattern(keywords::BASE64_URL),
        StringValidator::CapitalizePreformatted => pattern(keywords::CAPITALIZED),
        StringValidator::CreditCard => {
            JsCheck::Predicate(format!("{} && {LUHN}", test(keywords::CREDIT_CARD)?))
        }
        StringValidator::Date => {
            JsCheck::Predicate("!Number.isNaN(new Date(value).valueOf())".to_owned())
        }
        StringValidator::DateEpoch => JsCheck::Predicate(format!(
            "{} && Math.abs(Number.parseInt(value, 10)) <= {}",
            test(keywords::INTEGER)?,
            keywords::MAX_EPOCH_MILLIS
        )),
        StringValidator::DateIso => pattern(keywords::ISO_8601),
        StringValidator::Digits => pattern(keywords::DIGITS),
        StringValidator::Email => pattern(keywords::EMAIL),
        StringValidator::Hex => pattern(keywords::HEX),
        StringValidator::Integer => pattern(keywords::INTEGER),
        StringValidator::Ip => JsCheck::Predicate(format!(
            "{} || {}",
            test(keywords::IPV4)?,
            test(keywords::IPV6)?
        )),
        StringValidator::IpV4 => pattern(keywords::IPV4),
        StringValidator::IpV6 => pattern(keywords::IPV6),
        StringValidator::Json => JsCheck::Predicate(
            "(() => { try { JSON.parse(value); return true; } catch { return false; } })()"
                .to_owned(),
        ),
        StringValidator::LowerPreformatted => pattern(keywords::LOWER),
        StringValidator::NormalizeNFCPreformatted => normalized(NormalForm::Nfc),
        StringValidator::NormalizeNFDPreformatted => normalized(NormalForm::Nfd),
        StringValidator::NormalizeNFKCPreformatted => normalized(NormalForm::Nfkc),
        StringValidator::NormalizeNFKDPreformatted => normalized(NormalForm::Nfkd),
        StringValidator::Numeric => pattern(keywords::NUMERIC),
        StringValidator::Regex => JsCheck::Predicate(
            "(() => { try { new RegExp(value); return true; } catch { return false; } })()"
                .to_owned(),
        ),
        StringValidator::Semver => pattern(keywords::SEMVER),
        StringValidator::TrimPreformatted => pattern(&keywords::trimmed_pattern()),
        StringValidator::UpperPreformatted => pattern(keywords::UPPER),
        StringValidator::Url => JsCheck::Predicate("URL.canParse(value)".to_owned()),
        StringValidator::Uuid => pattern(keywords::UUID),
        StringValidator::UuidV1 => pattern(&keywords::uuid_version('1')),
        StringValidator::UuidV2 => pattern(&keywords::uuid_version('2')),
        StringValidator::UuidV3 => pattern(&keywords::uuid_version('3')),
        StringValidator::UuidV4 => pattern(&keywords::uuid_version('4')),
        StringValidator::UuidV5 => pattern(&keywords::uuid_version('5')),
        StringValidator::UuidV6 => pattern(&keywords::uuid_version('6')),
        StringValidator::UuidV7 => pattern(&keywords::uuid_version('7')),
        StringValidator::UuidV8 => pattern(&keywords::uuid_version('8')),
        StringValidator::Literal(literal) => {
            JsCheck::Predicate(format!("value === {}", string_literal(literal)?))
        }
        StringValidator::RegexLiteral(format) => pattern(&format.pattern()),
        StringValidator::Length(bound) => JsCheck::Length(LengthCheck::Exactly(
            bounds::length(bound).map_err(EvenframeError::config)?,
        )),
        StringValidator::MinLength(length) => JsCheck::Length(LengthCheck::AtLeast(*length)),
        StringValidator::MaxLength(length) => JsCheck::Length(LengthCheck::AtMost(*length)),
        StringValidator::NonEmpty => JsCheck::Length(LengthCheck::AtLeast(1)),
        StringValidator::StartsWith(prefix) => {
            JsCheck::Predicate(format!("value.startsWith({})", string_literal(prefix)?))
        }
        StringValidator::EndsWith(suffix) => {
            JsCheck::Predicate(format!("value.endsWith({})", string_literal(suffix)?))
        }
        StringValidator::Includes(substring) => {
            JsCheck::Predicate(format!("value.includes({})", string_literal(substring)?))
        }
        StringValidator::Trimmed => JsCheck::Predicate("value.trim() === value".to_owned()),
        StringValidator::Lowercased => {
            JsCheck::Predicate("value.toLowerCase() === value".to_owned())
        }
        StringValidator::Uppercased => {
            JsCheck::Predicate("value.toUpperCase() === value".to_owned())
        }
        StringValidator::Capitalized => {
            JsCheck::Predicate("value[0]?.toUpperCase() === value[0]".to_owned())
        }
        StringValidator::Uncapitalized => {
            JsCheck::Predicate("value[0]?.toLowerCase() === value[0]".to_owned())
        }
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
        | StringValidator::NormalizeNFKD
        | StringValidator::IntegerParse
        | StringValidator::NumericParse
        | StringValidator::DateParse
        | StringValidator::DateIsoParse
        | StringValidator::DateEpochParse
        | StringValidator::JsonParse
        | StringValidator::UrlParse => return Ok(None),
    };
    Ok(Some(check))
}

/// `runtime::is_multiple_of` as a boolean expression over the number
/// `value`. JavaScript writes a small or large number with an exponent, which
/// the place count corrects for, so it counts the places Rust does.
pub fn multiple_of_check(divisor: f64) -> String {
    format!(
        "((places) => {{ const scale = 10 ** places; return Math.round(value * scale) % Math.round({divisor} * scale) === 0; }})(Math.max(((text) => {{ const [mantissa, exponent = \"0\"] = text.split(\"e\"); return Math.max(0, (mantissa.split(\".\")[1] ?? \"\").length - Number(exponent)); }})(String(value)), {}))",
        runtime::decimal_places(divisor)
    )
}

fn decimal_literal(bound: &str) -> Result<String> {
    bounds::decimal(bound).map_err(EvenframeError::config)?;
    string_literal(bound)
}

/// A decimal check as a boolean expression over `value`, which needs
/// [`COMPARE_DECIMAL`] in scope.
pub fn decimal_check(validator: &BigDecimalValidator) -> Result<String> {
    let compare = |bound: &str, condition: &str| -> Result<String> {
        Ok(format!(
            "compareDecimal(String(value), {}) {condition}",
            decimal_literal(bound)?
        ))
    };
    Ok(match validator {
        BigDecimalValidator::GreaterThanBigDecimal(bound) => compare(bound, "> 0")?,
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(bound) => compare(bound, ">= 0")?,
        BigDecimalValidator::LessThanBigDecimal(bound) => compare(bound, "< 0")?,
        BigDecimalValidator::LessThanOrEqualToBigDecimal(bound) => compare(bound, "<= 0")?,
        BigDecimalValidator::BetweenBigDecimal(start, end) => {
            format!("{} && {}", compare(start, ">= 0")?, compare(end, "<= 0")?)
        }
        BigDecimalValidator::PositiveBigDecimal => compare("0", "> 0")?,
        BigDecimalValidator::NonNegativeBigDecimal => compare("0", ">= 0")?,
        BigDecimalValidator::NegativeBigDecimal => compare("0", "< 0")?,
        BigDecimalValidator::NonPositiveBigDecimal => compare("0", "<= 0")?,
    })
}

fn duration_literal(bound: &str) -> Result<String> {
    bounds::duration(bound)
        .map(|nanos| format!("{nanos}n"))
        .map_err(EvenframeError::config)
}

/// A duration check as a boolean expression over `value`, which needs
/// [`DURATION_NANOS`] in scope.
pub fn duration_check(validator: &DurationValidator) -> Result<String> {
    let compare = |condition: String| {
        format!("((nanos) => nanos !== null && {condition})(durationNanos(value))")
    };
    Ok(match validator {
        DurationValidator::GreaterThanDuration(bound) => {
            compare(format!("nanos > {}", duration_literal(bound)?))
        }
        DurationValidator::GreaterThanOrEqualToDuration(bound) => {
            compare(format!("nanos >= {}", duration_literal(bound)?))
        }
        DurationValidator::LessThanDuration(bound) => {
            compare(format!("nanos < {}", duration_literal(bound)?))
        }
        DurationValidator::LessThanOrEqualToDuration(bound) => {
            compare(format!("nanos <= {}", duration_literal(bound)?))
        }
        DurationValidator::BetweenDuration(start, end) => compare(format!(
            "nanos >= {} && nanos <= {}",
            duration_literal(start)?,
            duration_literal(end)?
        )),
    })
}

/// An exact decimal comparison, `compareDecimal(a, b)` returning -1, 0 or 1,
/// for generated code that bounds decimals held as strings.
pub const COMPARE_DECIMAL: &str = "const compareDecimal = (left: string, right: string): number => { \
const parse = (text: string) => { const negative = text.startsWith(\"-\"); const [whole, fraction = \"\"] = text.replace(/^-/, \"\").split(\".\"); \
const digits = whole.replace(/^0+/, \"\"); const decimals = fraction.replace(/0+$/, \"\"); \
return { negative: negative && (digits !== \"\" || decimals !== \"\"), digits, decimals }; }; \
const magnitude = (a: { digits: string; decimals: string }, b: { digits: string; decimals: string }): number => \
a.digits.length !== b.digits.length ? Math.sign(a.digits.length - b.digits.length) \
: a.digits !== b.digits ? (a.digits < b.digits ? -1 : 1) \
: a.decimals.padEnd(b.decimals.length, \"0\") === b.decimals.padEnd(a.decimals.length, \"0\") ? 0 \
: a.decimals.padEnd(b.decimals.length, \"0\") < b.decimals.padEnd(a.decimals.length, \"0\") ? -1 : 1; \
const a = parse(String(left)); const b = parse(String(right)); \
if (a.negative !== b.negative) return a.negative ? -1 : 1; \
return a.negative ? magnitude(b, a) : magnitude(a, b); };\n";

/// A duration in nanoseconds, `durationNanos(value)`, for generated code that
/// bounds durations held as serde's `{ secs, nanos }`, as numbers, or as
/// SurrealDB-style text (`1h30m`). Anything else gives `null`.
pub const DURATION_NANOS: &str = "const durationNanos = (value: unknown): bigint | null => { \
if (typeof value === \"bigint\") return value; if (typeof value === \"number\") return Number.isInteger(value) ? BigInt(value) : null; \
if (typeof value === \"object\" && value !== null && \"secs\" in value && \"nanos\" in value) \
return typeof value.secs === \"number\" && typeof value.nanos === \"number\" ? BigInt(value.secs) * 1000000000n + BigInt(value.nanos) : null; \
const text = String(value); if (!/^(?:\\d+(?:ns|us|µs|ms|s|m|h|d|w|y))+$/.test(text)) return null; \
const units: Record<string, bigint> = { ns: 1n, us: 1000n, \"µs\": 1000n, ms: 1000000n, s: 1000000000n, m: 60000000000n, h: 3600000000000n, d: 86400000000000n, w: 604800000000000n, y: 31536000000000000n }; \
let total = 0n; for (const [, amount, unit] of text.matchAll(/(\\d+)(ns|us|µs|ms|s|m|h|d|w|y)/g)) total += BigInt(amount) * units[unit]; return total; };\n";
