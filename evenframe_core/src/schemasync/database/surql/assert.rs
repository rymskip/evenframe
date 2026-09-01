use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator, parse_duration_to_nanos,
};
use tracing::{debug, trace};

// ---------------------------------------------------------------------------
// Embedded-JavaScript ASSERT bodies (require the server `--allow-scripting` flag)
//
// Each body runs inside `function($value) { const v = arguments[0]; <body> }`
// and must `return` a boolean. They mirror the corresponding Rust validation
// logic in `validator.rs::Validator::get_validation_logic_tokens`.
// ---------------------------------------------------------------------------

/// Luhn checksum over the digit characters of the value.
const CREDIT_CARD_JS: &str = "if (typeof v !== 'string' && typeof v !== 'number') return false; \
const d = String(v).replace(/[^0-9]/g, ''); \
if (d.length < 13 || d.length > 19) return false; \
let sum = 0; let alt = false; \
for (let i = d.length - 1; i >= 0; i--) { let n = d.charCodeAt(i) - 48; if (alt) { n *= 2; if (n > 9) n -= 9; } sum += n; alt = !alt; } \
return sum % 10 === 0;";

/// Parseable JSON string.
const JSON_JS: &str = "try { JSON.parse(v); return true; } catch (e) { return false; }";

/// First character is an uppercase letter and the remainder contains no
/// non-lowercase letters.
const CAPITALIZED_JS: &str = "if (typeof v !== 'string' || v.length === 0) return false; \
const a = Array.from(v); const f = a[0]; \
if (!(f === f.toUpperCase() && f !== f.toLowerCase())) return false; \
for (let i = 1; i < a.length; i++) { const c = a[i]; if (c.toLowerCase() !== c.toUpperCase() && c !== c.toLowerCase()) return false; } \
return true;";

/// First character is a lowercase letter.
const UNCAPITALIZED_JS: &str = "if (typeof v !== 'string' || v.length === 0) return false; \
const f = Array.from(v)[0]; return f === f.toLowerCase() && f !== f.toUpperCase();";

/// Escape a string for embedding inside a double-quoted SurrealQL string literal.
fn escape_surql_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Convert a `%Y-%m-%d` date string into a SurrealDB datetime literal
/// (`d'YYYY-MM-DDT00:00:00Z'`). Returns `None` if the string can't be parsed.
fn datetime_literal(date_str: &str) -> Option<String> {
    let d = chrono::NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d").ok()?;
    Some(format!("d'{}T00:00:00Z'", d.format("%Y-%m-%d")))
}

/// Convert a decimal string into a SurrealDB decimal literal (`<n>dec`).
/// Returns `None` if the string isn't a valid decimal number.
fn decimal_literal(s: &str) -> Option<String> {
    let t = s.trim();
    t.parse::<f64>().ok()?;
    Some(format!("{t}dec"))
}

/// Validate an integer string and return its trimmed form for use as a SurrealDB
/// integer literal. Returns `None` if it isn't a valid integer.
fn int_literal(s: &str) -> Option<String> {
    let t = s.trim();
    t.parse::<i128>().ok()?;
    Some(t.to_string())
}

/// Convert a SurrealDB-style duration string (e.g. `"1h"`, `"30m500ms"`) into a
/// `duration::from_nanos(N)` literal. Returns `None` if it can't be parsed.
fn duration_literal(s: &str) -> Option<String> {
    let nanos = parse_duration_to_nanos(s)?;
    Some(format!("duration::from_nanos({nanos})"))
}

/// Build an embedded-JavaScript ASSERT expression. The field value is passed in
/// as the sole argument and read back via `arguments[0]`. Emitted single-line so
/// it round-trips stably against SurrealDB's stored form.
///
/// Requires the SurrealDB server to be started with `--allow-scripting`.
fn js_assert(value_var: &str, body: &str) -> String {
    format!("function({value_var}) {{ const v = arguments[0]; {body} }}")
}

/// SurrealDB regex matching a UUID whose version nibble equals `version`.
fn uuid_version_regex(value_var: &str, version: char) -> String {
    format!(
        "string::matches({value_var}, \"^[0-9a-fA-F]{{8}}-[0-9a-fA-F]{{4}}-{version}[0-9a-fA-F]{{3}}-[0-9a-fA-F]{{4}}-[0-9a-fA-F]{{12}}$\")"
    )
}

/// Generate an ASSERT clause from a field's validators.
///
/// Native SurrealQL is used wherever possible. Validators whose semantics have
/// no native SurrealDB function (checksums, JSON/Unicode parsing, NaN-free
/// checks) are emitted as embedded-JavaScript functions, but only when
/// `allow_scripting` is true (the server must run with `--allow-scripting`);
/// otherwise those validators contribute no assertion.
///
/// Pure transformation morphs (`Trim`, `Lower`, `Capitalize`, `*Parse`, …) and
/// the no-op `String`/`Regex` (no pattern) variants never produce an assertion —
/// an ASSERT validates a stored value, it cannot transform it.
pub fn generate_assert_from_validators(
    validators: &[Validator],
    value_var: &str,
    allow_scripting: bool,
) -> String {
    debug!(
        "Generating ASSERT clauses from {} validators for variable: {} (allow_scripting={})",
        validators.len(),
        value_var,
        allow_scripting
    );
    let mut assertions = Vec::new();

    for (i, validator) in validators.iter().enumerate() {
        trace!(
            "Processing validator {} of {}: {:?}",
            i + 1,
            validators.len(),
            validator
        );
        match validator {
            // ---------------------------------------------------------------
            // String validators
            // ---------------------------------------------------------------
            Validator::StringValidator(sv) => match sv {
                // Native string predicate functions
                StringValidator::Email => {
                    assertions.push(format!("string::is_email({value_var})"))
                }
                StringValidator::Alpha => {
                    assertions.push(format!("string::is_alpha({value_var})"))
                }
                StringValidator::Alphanumeric => {
                    assertions.push(format!("string::is_alphanum({value_var})"))
                }
                StringValidator::Hex => {
                    assertions.push(format!("string::is_hexadecimal({value_var})"))
                }
                StringValidator::Ip => assertions.push(format!("string::is_ip({value_var})")),
                StringValidator::IpV4 => assertions.push(format!("string::is_ipv4({value_var})")),
                StringValidator::IpV6 => assertions.push(format!("string::is_ipv6({value_var})")),
                StringValidator::Url => assertions.push(format!("string::is_url({value_var})")),
                StringValidator::Uuid => assertions.push(format!("string::is_uuid({value_var})")),
                StringValidator::Semver => {
                    assertions.push(format!("string::is_semver({value_var})"))
                }
                StringValidator::Numeric => {
                    assertions.push(format!("string::is_numeric({value_var})"))
                }
                // Pure digits only (string::is_numeric also accepts signs/decimals).
                StringValidator::Digits => {
                    assertions.push(format!("string::matches({value_var}, \"^[0-9]+$\")"))
                }
                StringValidator::Integer | StringValidator::DateEpoch => {
                    assertions.push(format!("string::matches({value_var}, \"^[+-]?[0-9]+$\")"))
                }
                StringValidator::Date | StringValidator::DateIso => {
                    assertions.push(format!("string::is_datetime({value_var})"))
                }

                // Length / content
                StringValidator::MinLength(len) => {
                    assertions.push(format!("string::len({value_var}) >= {len}"))
                }
                StringValidator::MaxLength(len) => {
                    assertions.push(format!("string::len({value_var}) <= {len}"))
                }
                StringValidator::Length(len) => {
                    assertions.push(format!("string::len({value_var}) = {len}"))
                }
                StringValidator::NonEmpty => {
                    assertions.push(format!("string::len({value_var}) > 0"))
                }
                StringValidator::StartsWith(prefix) => assertions.push(format!(
                    "string::starts_with({value_var}, \"{}\")",
                    escape_surql_string(prefix)
                )),
                StringValidator::EndsWith(suffix) => assertions.push(format!(
                    "string::ends_with({value_var}, \"{}\")",
                    escape_surql_string(suffix)
                )),
                StringValidator::Includes(substring) => assertions.push(format!(
                    "string::contains({value_var}, \"{}\")",
                    escape_surql_string(substring)
                )),

                // Validation-only formatting checks (the value already equals its
                // transformed form). `*Preformatted` twins share the same check.
                StringValidator::Trimmed | StringValidator::TrimPreformatted => {
                    assertions.push(format!("{value_var} = string::trim({value_var})"))
                }
                StringValidator::Lowercased | StringValidator::LowerPreformatted => {
                    assertions.push(format!("{value_var} = string::lowercase({value_var})"))
                }
                StringValidator::Uppercased | StringValidator::UpperPreformatted => {
                    assertions.push(format!("{value_var} = string::uppercase({value_var})"))
                }
                StringValidator::Literal(literal) => assertions.push(format!(
                    "{value_var} = \"{}\"",
                    escape_surql_string(literal)
                )),
                StringValidator::RegexLiteral(format_variant) => assertions.push(format!(
                    "string::matches({value_var}, \"{}\")",
                    // Format regexes contain backslashes (\d, \., \s, …). They must
                    // be escaped for the SurrealQL string literal, or SurrealDB
                    // rejects the DEFINE FIELD with "invalid escape sequence".
                    escape_surql_string(format_variant.to_owned().into_regex().as_str())
                )),

                // Base64 — native regex plus the length-multiple-of-4 rule.
                StringValidator::Base64 => assertions.push(format!(
                    "string::matches({value_var}, \"^[A-Za-z0-9+/]*={{0,2}}$\") AND string::len({value_var}) % 4 = 0"
                )),
                StringValidator::Base64Url => assertions.push(format!(
                    "string::matches({value_var}, \"^[A-Za-z0-9_-]*={{0,2}}$\") AND string::len({value_var}) % 4 = 0"
                )),

                // UUID versions — native regex on the version nibble.
                StringValidator::UuidV1 => assertions.push(uuid_version_regex(value_var, '1')),
                StringValidator::UuidV2 => assertions.push(uuid_version_regex(value_var, '2')),
                StringValidator::UuidV3 => assertions.push(uuid_version_regex(value_var, '3')),
                StringValidator::UuidV4 => assertions.push(uuid_version_regex(value_var, '4')),
                StringValidator::UuidV5 => assertions.push(uuid_version_regex(value_var, '5')),
                StringValidator::UuidV6 => assertions.push(uuid_version_regex(value_var, '6')),
                StringValidator::UuidV7 => assertions.push(uuid_version_regex(value_var, '7')),
                StringValidator::UuidV8 => assertions.push(uuid_version_regex(value_var, '8')),

                // Embedded-JavaScript checks (only when scripting is permitted).
                StringValidator::CreditCard if allow_scripting => {
                    assertions.push(js_assert(value_var, CREDIT_CARD_JS))
                }
                StringValidator::Json if allow_scripting => {
                    assertions.push(js_assert(value_var, JSON_JS))
                }
                StringValidator::NormalizeNFCPreformatted if allow_scripting => assertions
                    .push(js_assert(value_var, "return v === v.normalize('NFC');")),
                StringValidator::NormalizeNFDPreformatted if allow_scripting => assertions
                    .push(js_assert(value_var, "return v === v.normalize('NFD');")),
                StringValidator::NormalizeNFKCPreformatted if allow_scripting => assertions
                    .push(js_assert(value_var, "return v === v.normalize('NFKC');")),
                StringValidator::NormalizeNFKDPreformatted if allow_scripting => assertions
                    .push(js_assert(value_var, "return v === v.normalize('NFKD');")),
                StringValidator::Capitalized if allow_scripting => {
                    assertions.push(js_assert(value_var, CAPITALIZED_JS))
                }
                StringValidator::Uncapitalized if allow_scripting => {
                    assertions.push(js_assert(value_var, UNCAPITALIZED_JS))
                }

                // Transformations / no-ops / scripting-disabled fallbacks: no assertion.
                _ => trace!(
                    "String validator {:?} produces no SurrealDB assertion (transformation, no-op, or scripting disabled)",
                    sv
                ),
            },

            // ---------------------------------------------------------------
            // Number validators
            // ---------------------------------------------------------------
            Validator::NumberValidator(nv) => match nv {
                NumberValidator::GreaterThan(value) => {
                    assertions.push(format!("{value_var} > {}", value.0))
                }
                NumberValidator::GreaterThanOrEqualTo(value) => {
                    assertions.push(format!("{value_var} >= {}", value.0))
                }
                NumberValidator::LessThan(value) => {
                    assertions.push(format!("{value_var} < {}", value.0))
                }
                NumberValidator::LessThanOrEqualTo(value) => {
                    assertions.push(format!("{value_var} <= {}", value.0))
                }
                NumberValidator::Between(start, end) => assertions.push(format!(
                    "{value_var} >= {} AND {value_var} <= {}",
                    start.0, end.0
                )),
                NumberValidator::Int => assertions.push(format!("type::is_int({value_var})")),
                NumberValidator::Positive => assertions.push(format!("{value_var} > 0")),
                NumberValidator::NonNegative => assertions.push(format!("{value_var} >= 0")),
                NumberValidator::Negative => assertions.push(format!("{value_var} < 0")),
                NumberValidator::NonPositive => assertions.push(format!("{value_var} <= 0")),
                NumberValidator::MultipleOf(value) => {
                    assertions.push(format!("{value_var} % {} = 0", value.0))
                }
                NumberValidator::Uint8 => assertions.push(format!(
                    "type::is_int({value_var}) AND {value_var} >= 0 AND {value_var} <= 255"
                )),
                // NaN never equals itself, so self-equality is a native NaN guard.
                NumberValidator::NonNaN => assertions.push(format!("{value_var} = {value_var}")),
                NumberValidator::Finite if allow_scripting => assertions.push(js_assert(
                    value_var,
                    "return typeof v === 'number' ? Number.isFinite(v) : true;",
                )),
                NumberValidator::Finite => trace!(
                    "Number validator Finite produces no assertion (scripting disabled)"
                ),
            },

            // ---------------------------------------------------------------
            // Array validators
            // ---------------------------------------------------------------
            Validator::ArrayValidator(av) => match av {
                ArrayValidator::MinItems(count) => {
                    assertions.push(format!("array::len({value_var}) >= {count}"))
                }
                ArrayValidator::MaxItems(count) => {
                    assertions.push(format!("array::len({value_var}) <= {count}"))
                }
                ArrayValidator::ItemsCount(count) => {
                    assertions.push(format!("array::len({value_var}) = {count}"))
                }
            },

            // ---------------------------------------------------------------
            // Date validators (field type is `datetime`)
            // ---------------------------------------------------------------
            Validator::DateValidator(dv) => match dv {
                DateValidator::ValidDate => {
                    assertions.push(format!("type::is_datetime({value_var})"))
                }
                DateValidator::GreaterThanDate(s) => {
                    push_date(&mut assertions, value_var, ">", s)
                }
                DateValidator::GreaterThanOrEqualToDate(s) => {
                    push_date(&mut assertions, value_var, ">=", s)
                }
                DateValidator::LessThanDate(s) => push_date(&mut assertions, value_var, "<", s),
                DateValidator::LessThanOrEqualToDate(s) => {
                    push_date(&mut assertions, value_var, "<=", s)
                }
                DateValidator::BetweenDate(a, b) => {
                    match (datetime_literal(a), datetime_literal(b)) {
                        (Some(la), Some(lb)) => assertions.push(format!(
                            "{value_var} >= {la} AND {value_var} <= {lb}"
                        )),
                        _ => trace!("DateValidator::BetweenDate has unparseable bound: {a:?}, {b:?}"),
                    }
                }
            },

            // ---------------------------------------------------------------
            // BigInt validators (field type is `int`)
            // ---------------------------------------------------------------
            Validator::BigIntValidator(biv) => match biv {
                BigIntValidator::GreaterThanBigInt(s) => {
                    push_int(&mut assertions, value_var, ">", s)
                }
                BigIntValidator::GreaterThanOrEqualToBigInt(s) => {
                    push_int(&mut assertions, value_var, ">=", s)
                }
                BigIntValidator::LessThanBigInt(s) => {
                    push_int(&mut assertions, value_var, "<", s)
                }
                BigIntValidator::LessThanOrEqualToBigInt(s) => {
                    push_int(&mut assertions, value_var, "<=", s)
                }
                BigIntValidator::BetweenBigInt(a, b) => match (int_literal(a), int_literal(b)) {
                    (Some(la), Some(lb)) => {
                        assertions.push(format!("{value_var} >= {la} AND {value_var} <= {lb}"))
                    }
                    _ => trace!("BigIntValidator::BetweenBigInt has unparseable bound: {a:?}, {b:?}"),
                },
                BigIntValidator::PositiveBigInt => assertions.push(format!("{value_var} > 0")),
                BigIntValidator::NonNegativeBigInt => assertions.push(format!("{value_var} >= 0")),
                BigIntValidator::NegativeBigInt => assertions.push(format!("{value_var} < 0")),
                BigIntValidator::NonPositiveBigInt => assertions.push(format!("{value_var} <= 0")),
            },

            // ---------------------------------------------------------------
            // BigDecimal validators (field type is `decimal`)
            // ---------------------------------------------------------------
            Validator::BigDecimalValidator(bdv) => match bdv {
                BigDecimalValidator::GreaterThanBigDecimal(s) => {
                    push_decimal(&mut assertions, value_var, ">", s)
                }
                BigDecimalValidator::GreaterThanOrEqualToBigDecimal(s) => {
                    push_decimal(&mut assertions, value_var, ">=", s)
                }
                BigDecimalValidator::LessThanBigDecimal(s) => {
                    push_decimal(&mut assertions, value_var, "<", s)
                }
                BigDecimalValidator::LessThanOrEqualToBigDecimal(s) => {
                    push_decimal(&mut assertions, value_var, "<=", s)
                }
                BigDecimalValidator::BetweenBigDecimal(a, b) => {
                    match (decimal_literal(a), decimal_literal(b)) {
                        (Some(la), Some(lb)) => assertions.push(format!(
                            "{value_var} >= {la} AND {value_var} <= {lb}"
                        )),
                        _ => trace!(
                            "BigDecimalValidator::BetweenBigDecimal has unparseable bound: {a:?}, {b:?}"
                        ),
                    }
                }
                BigDecimalValidator::PositiveBigDecimal => {
                    assertions.push(format!("{value_var} > 0dec"))
                }
                BigDecimalValidator::NonNegativeBigDecimal => {
                    assertions.push(format!("{value_var} >= 0dec"))
                }
                BigDecimalValidator::NegativeBigDecimal => {
                    assertions.push(format!("{value_var} < 0dec"))
                }
                BigDecimalValidator::NonPositiveBigDecimal => {
                    assertions.push(format!("{value_var} <= 0dec"))
                }
            },

            // ---------------------------------------------------------------
            // Duration validators (field type is `duration`)
            // ---------------------------------------------------------------
            Validator::DurationValidator(dv) => match dv {
                DurationValidator::GreaterThanDuration(s) => {
                    push_duration(&mut assertions, value_var, ">", s)
                }
                DurationValidator::GreaterThanOrEqualToDuration(s) => {
                    push_duration(&mut assertions, value_var, ">=", s)
                }
                DurationValidator::LessThanDuration(s) => {
                    push_duration(&mut assertions, value_var, "<", s)
                }
                DurationValidator::LessThanOrEqualToDuration(s) => {
                    push_duration(&mut assertions, value_var, "<=", s)
                }
                DurationValidator::BetweenDuration(a, b) => {
                    match (duration_literal(a), duration_literal(b)) {
                        (Some(la), Some(lb)) => assertions.push(format!(
                            "{value_var} >= {la} AND {value_var} <= {lb}"
                        )),
                        _ => trace!(
                            "DurationValidator::BetweenDuration has unparseable bound: {a:?}, {b:?}"
                        ),
                    }
                }
            },
        }
    }

    // Join all assertions with AND
    let result = if assertions.is_empty() {
        debug!(
            "No valid assertions generated from {} validators",
            validators.len()
        );
        String::new()
    } else {
        debug!(
            "Generated {} valid assertions from {} validators",
            assertions.len(),
            validators.len()
        );
        trace!("Final assertions: {:?}", assertions);
        assertions.join(" AND ")
    };

    debug!(
        "ASSERT clause generation completed, result length: {}",
        result.len()
    );
    result
}

fn push_date(assertions: &mut Vec<String>, value_var: &str, op: &str, bound: &str) {
    match datetime_literal(bound) {
        Some(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        None => trace!("DateValidator bound is unparseable: {bound:?}"),
    }
}

fn push_int(assertions: &mut Vec<String>, value_var: &str, op: &str, bound: &str) {
    match int_literal(bound) {
        Some(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        None => trace!("BigIntValidator bound is unparseable: {bound:?}"),
    }
}

fn push_decimal(assertions: &mut Vec<String>, value_var: &str, op: &str, bound: &str) {
    match decimal_literal(bound) {
        Some(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        None => trace!("BigDecimalValidator bound is unparseable: {bound:?}"),
    }
}

fn push_duration(assertions: &mut Vec<String>, value_var: &str, op: &str, bound: &str) {
    match duration_literal(bound) {
        Some(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        None => trace!("DurationValidator bound is unparseable: {bound:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemasync::mockmake::format::Format;
    use ordered_float::OrderedFloat;

    fn gen_assert(v: Validator) -> String {
        generate_assert_from_validators(&[v], "$value", true)
    }

    fn gen_assert_no_js(v: Validator) -> String {
        generate_assert_from_validators(&[v], "$value", false)
    }

    #[test]
    fn string_native_predicates() {
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Email)),
            "string::is_email($value)"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Url)),
            "string::is_url($value)"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Numeric)),
            "string::is_numeric($value)"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Digits)),
            "string::matches($value, \"^[0-9]+$\")"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Integer)),
            "string::matches($value, \"^[+-]?[0-9]+$\")"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Date)),
            "string::is_datetime($value)"
        );
    }

    #[test]
    fn string_length_and_content_escape() {
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::MinLength(3))),
            "string::len($value) >= 3"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::NonEmpty)),
            "string::len($value) > 0"
        );
        // Quotes inside the argument must be escaped.
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::StartsWith(
                "a\"b".to_string()
            ))),
            "string::starts_with($value, \"a\\\"b\")"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Literal(
                "x".to_string()
            ))),
            "$value = \"x\""
        );
    }

    #[test]
    fn string_preformatted_and_base64_and_uuid() {
        assert_eq!(
            gen_assert(Validator::StringValidator(
                StringValidator::LowerPreformatted
            )),
            "$value = string::lowercase($value)"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(
                StringValidator::TrimPreformatted
            )),
            "$value = string::trim($value)"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Base64)),
            "string::matches($value, \"^[A-Za-z0-9+/]*={0,2}$\") AND string::len($value) % 4 = 0"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::UuidV4)),
            "string::matches($value, \"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-4[0-9a-fA-F]{3}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$\")"
        );
    }

    #[test]
    fn string_js_variants_gated_on_scripting() {
        let cc = gen_assert(Validator::StringValidator(StringValidator::CreditCard));
        assert!(cc.starts_with("function($value) { const v = arguments[0]; "));
        assert!(cc.contains("sum % 10 === 0"));
        // No newlines — must be single-line for stable round-trip.
        assert!(!cc.contains('\n'));

        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Json)),
            "function($value) { const v = arguments[0]; try { JSON.parse(v); return true; } catch (e) { return false; } }"
        );

        // Disabling scripting drops the JS-only assertions entirely.
        assert_eq!(
            gen_assert_no_js(Validator::StringValidator(StringValidator::CreditCard)),
            ""
        );
        assert_eq!(
            gen_assert_no_js(Validator::StringValidator(StringValidator::Json)),
            ""
        );
        assert_eq!(
            gen_assert_no_js(Validator::StringValidator(StringValidator::Capitalized)),
            ""
        );
    }

    #[test]
    fn string_transformations_produce_nothing() {
        for v in [
            StringValidator::String,
            StringValidator::Regex,
            StringValidator::Trim,
            StringValidator::Lower,
            StringValidator::Upper,
            StringValidator::Capitalize,
            StringValidator::UrlParse,
            StringValidator::IntegerParse,
            StringValidator::JsonParse,
            StringValidator::DateParse,
            StringValidator::Normalize,
        ] {
            assert_eq!(gen_assert(Validator::StringValidator(v)), "");
        }
    }

    #[test]
    fn regex_literal_uses_matches() {
        let out = gen_assert(Validator::StringValidator(StringValidator::RegexLiteral(
            Format::Email,
        )));
        assert!(out.starts_with("string::matches($value, \""));
    }

    #[test]
    fn number_validators() {
        assert_eq!(
            gen_assert(Validator::NumberValidator(NumberValidator::GreaterThan(
                OrderedFloat(5.0)
            ))),
            "$value > 5"
        );
        assert_eq!(
            gen_assert(Validator::NumberValidator(NumberValidator::Between(
                OrderedFloat(1.0),
                OrderedFloat(10.0)
            ))),
            "$value >= 1 AND $value <= 10"
        );
        assert_eq!(
            gen_assert(Validator::NumberValidator(NumberValidator::Int)),
            "type::is_int($value)"
        );
        assert_eq!(
            gen_assert(Validator::NumberValidator(NumberValidator::Uint8)),
            "type::is_int($value) AND $value >= 0 AND $value <= 255"
        );
        assert_eq!(
            gen_assert(Validator::NumberValidator(NumberValidator::NonNaN)),
            "$value = $value"
        );
        assert_eq!(
            gen_assert(Validator::NumberValidator(NumberValidator::Finite)),
            "function($value) { const v = arguments[0]; return typeof v === 'number' ? Number.isFinite(v) : true; }"
        );
        assert_eq!(
            gen_assert_no_js(Validator::NumberValidator(NumberValidator::Finite)),
            ""
        );
    }

    #[test]
    fn array_validators() {
        assert_eq!(
            gen_assert(Validator::ArrayValidator(ArrayValidator::MinItems(2))),
            "array::len($value) >= 2"
        );
        assert_eq!(
            gen_assert(Validator::ArrayValidator(ArrayValidator::ItemsCount(4))),
            "array::len($value) = 4"
        );
    }

    #[test]
    fn date_validators() {
        assert_eq!(
            gen_assert(Validator::DateValidator(DateValidator::ValidDate)),
            "type::is_datetime($value)"
        );
        assert_eq!(
            gen_assert(Validator::DateValidator(DateValidator::GreaterThanDate(
                "2024-01-01".to_string()
            ))),
            "$value > d'2024-01-01T00:00:00Z'"
        );
        assert_eq!(
            gen_assert(Validator::DateValidator(DateValidator::BetweenDate(
                "2024-01-01".to_string(),
                "2024-12-31".to_string()
            ))),
            "$value >= d'2024-01-01T00:00:00Z' AND $value <= d'2024-12-31T00:00:00Z'"
        );
        // Unparseable bound yields no assertion.
        assert_eq!(
            gen_assert(Validator::DateValidator(DateValidator::GreaterThanDate(
                "not-a-date".to_string()
            ))),
            ""
        );
    }

    #[test]
    fn bigint_validators() {
        assert_eq!(
            gen_assert(Validator::BigIntValidator(
                BigIntValidator::GreaterThanBigInt("100".to_string())
            )),
            "$value > 100"
        );
        assert_eq!(
            gen_assert(Validator::BigIntValidator(BigIntValidator::PositiveBigInt)),
            "$value > 0"
        );
    }

    #[test]
    fn bigdecimal_validators() {
        assert_eq!(
            gen_assert(Validator::BigDecimalValidator(
                BigDecimalValidator::GreaterThanBigDecimal("3.14".to_string())
            )),
            "$value > 3.14dec"
        );
        assert_eq!(
            gen_assert(Validator::BigDecimalValidator(
                BigDecimalValidator::PositiveBigDecimal
            )),
            "$value > 0dec"
        );
    }

    #[test]
    fn duration_validators() {
        // 1h = 3_600_000_000_000 ns
        assert_eq!(
            gen_assert(Validator::DurationValidator(
                DurationValidator::GreaterThanDuration("1h".to_string())
            )),
            "$value > duration::from_nanos(3600000000000)"
        );
    }

    #[test]
    fn multiple_validators_joined_with_and() {
        let out = generate_assert_from_validators(
            &[
                Validator::StringValidator(StringValidator::NonEmpty),
                Validator::StringValidator(StringValidator::MaxLength(10)),
            ],
            "$value",
            true,
        );
        assert_eq!(out, "string::len($value) > 0 AND string::len($value) <= 10");
    }
}
