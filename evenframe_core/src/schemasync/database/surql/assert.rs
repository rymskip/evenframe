use crate::validator::bounds;
use crate::validator::keywords::{self, NormalForm};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use tracing::{debug, error, trace};

// ---------------------------------------------------------------------------
// Embedded-JavaScript ASSERT bodies (require the server `--allow-scripting` flag)
//
// Each body runs inside `function($value) { const v = arguments[0]; <body> }`
// and must `return` a boolean. They are ArkType's and Effect's own predicates.
// ---------------------------------------------------------------------------

/// validator.js's `isLuhnNumber`, which ArkType's `string.creditCard` uses
/// after its issuer pattern.
const LUHN_JS: &str = "const d = v.replace(/[ -]+/g, ''); let sum = 0; let double = false; \
for (let i = d.length - 1; i >= 0; i--) { let n = parseInt(d.charAt(i), 10); \
if (double) { n *= 2; sum += n >= 10 ? (n % 10) + 1 : n; } else { sum += n; } double = !double; } \
return sum % 10 === 0;";

/// ArkType's `string.json`.
const JSON_JS: &str = "try { JSON.parse(v); return true; } catch (e) { return false; }";

/// ArkType's `string.regex`.
const REGEX_JS: &str = "try { new RegExp(v); return true; } catch (e) { return false; }";

/// ArkType's `string.date`: a string JavaScript's `Date` can parse.
const PARSABLE_DATE_JS: &str = "return !Number.isNaN(new Date(v).valueOf());";

/// Effect's `capitalized`.
const CAPITALIZED_JS: &str = "return v[0]?.toUpperCase() === v[0];";

/// Effect's `uncapitalized`.
const UNCAPITALIZED_JS: &str = "return v[0]?.toLowerCase() === v[0];";

/// Escape a string for embedding inside a double-quoted SurrealQL string literal.
fn escape_surql_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `pattern` as a JavaScript string literal.
fn js_string(pattern: &str) -> String {
    serde_json::to_string(pattern).unwrap_or_else(|failure| {
        error!("cannot encode {pattern:?} for an ASSERT script: {failure}");
        "\"\"".to_owned()
    })
}

/// A native regex assertion with one of the shared keyword patterns.
fn matches(value_var: &str, pattern: &str) -> String {
    format!(
        "string::matches({value_var}, \"{}\")",
        escape_surql_string(pattern)
    )
}

/// A date bound as a SurrealDB datetime literal (`d'YYYY-MM-DDT00:00:00Z'`).
fn datetime_literal(bound: &str) -> Result<String, String> {
    bounds::date(bound).map(|instant| format!("d'{}'", instant.format("%Y-%m-%dT%H:%M:%SZ")))
}

/// A decimal bound as a SurrealDB decimal literal (`<n>dec`), never through a
/// float.
fn decimal_literal(bound: &str) -> Result<String, String> {
    bounds::decimal(bound).map(|_| format!("{}dec", bound.trim()))
}

/// An integer bound as a SurrealDB integer literal.
fn int_literal(bound: &str) -> Result<String, String> {
    bounds::big_int(bound).map(|number| number.to_string())
}

/// A duration bound as a `duration::from_nanos(N)` literal.
fn duration_literal(bound: &str) -> Result<String, String> {
    bounds::duration(bound).map(|nanos| format!("duration::from_nanos({nanos})"))
}

/// Build an embedded-JavaScript ASSERT expression. The field value is passed in
/// as the sole argument and read back via `arguments[0]`. Emitted single-line so
/// it round-trips stably against SurrealDB's stored form.
///
/// Requires the SurrealDB server to be started with `--allow-scripting`.
fn js_assert(value_var: &str, body: &str) -> String {
    format!("function({value_var}) {{ const v = arguments[0]; {body} }}")
}

/// The assertion for a string validator. ArkType-named keywords use the
/// shared patterns natively; predicates without a native form run as
/// JavaScript when scripting is allowed. Morphs transform input rather than
/// constrain storage, so they assert nothing.
fn string_assertion(
    validator: &StringValidator,
    value_var: &str,
    allow_scripting: bool,
) -> Option<String> {
    let script = |body: &str| allow_scripting.then(|| js_assert(value_var, body));
    let normalized_js =
        |form: NormalForm| script(&format!("return v.normalize('{}') === v;", form.name()));
    match validator {
        StringValidator::Alpha => Some(matches(value_var, keywords::ALPHA)),
        StringValidator::Alphanumeric => Some(matches(value_var, keywords::ALPHANUMERIC)),
        StringValidator::Base64 => Some(matches(value_var, keywords::BASE64)),
        StringValidator::Base64Url => Some(matches(value_var, keywords::BASE64_URL)),
        StringValidator::CapitalizePreformatted => Some(matches(value_var, keywords::CAPITALIZED)),
        StringValidator::CreditCard => {
            let issuer = matches(value_var, keywords::CREDIT_CARD);
            Some(match script(LUHN_JS) {
                Some(luhn) => format!("{issuer} AND {luhn}"),
                None => issuer,
            })
        }
        StringValidator::Date => {
            script(PARSABLE_DATE_JS).or_else(|| Some(format!("string::is_datetime({value_var})")))
        }
        StringValidator::DateEpoch => Some(format!(
            "{} AND math::abs(<int> {value_var}) <= {}",
            matches(value_var, keywords::INTEGER),
            keywords::MAX_EPOCH_MILLIS
        )),
        StringValidator::DateIso => script(&format!(
            "return new RegExp({}).test(v);",
            js_string(keywords::ISO_8601)
        ))
        .or_else(|| Some(format!("string::is_datetime({value_var})"))),
        StringValidator::Digits => Some(matches(value_var, keywords::DIGITS)),
        StringValidator::Email => Some(matches(value_var, keywords::EMAIL)),
        StringValidator::Hex => Some(matches(value_var, keywords::HEX)),
        StringValidator::Integer => Some(matches(value_var, keywords::INTEGER)),
        StringValidator::Ip => Some(format!(
            "({} OR {})",
            matches(value_var, keywords::IPV4),
            matches(value_var, keywords::IPV6)
        )),
        StringValidator::IpV4 => Some(matches(value_var, keywords::IPV4)),
        StringValidator::IpV6 => Some(matches(value_var, keywords::IPV6)),
        StringValidator::Json => script(JSON_JS),
        StringValidator::LowerPreformatted => Some(matches(value_var, keywords::LOWER)),
        StringValidator::NormalizeNFCPreformatted => normalized_js(NormalForm::Nfc),
        StringValidator::NormalizeNFDPreformatted => normalized_js(NormalForm::Nfd),
        StringValidator::NormalizeNFKCPreformatted => normalized_js(NormalForm::Nfkc),
        StringValidator::NormalizeNFKDPreformatted => normalized_js(NormalForm::Nfkd),
        StringValidator::Numeric => Some(matches(value_var, keywords::NUMERIC)),
        StringValidator::Regex => script(REGEX_JS),
        StringValidator::Semver => Some(matches(value_var, keywords::SEMVER)),
        StringValidator::TrimPreformatted => Some(matches(value_var, &keywords::trimmed_pattern())),
        StringValidator::UpperPreformatted => Some(matches(value_var, keywords::UPPER)),
        StringValidator::Url => Some(format!("string::is_url({value_var})")),
        StringValidator::Uuid => Some(matches(value_var, keywords::UUID)),
        StringValidator::UuidV1 => Some(matches(value_var, &keywords::uuid_version('1'))),
        StringValidator::UuidV2 => Some(matches(value_var, &keywords::uuid_version('2'))),
        StringValidator::UuidV3 => Some(matches(value_var, &keywords::uuid_version('3'))),
        StringValidator::UuidV4 => Some(matches(value_var, &keywords::uuid_version('4'))),
        StringValidator::UuidV5 => Some(matches(value_var, &keywords::uuid_version('5'))),
        StringValidator::UuidV6 => Some(matches(value_var, &keywords::uuid_version('6'))),
        StringValidator::UuidV7 => Some(matches(value_var, &keywords::uuid_version('7'))),
        StringValidator::UuidV8 => Some(matches(value_var, &keywords::uuid_version('8'))),
        StringValidator::Literal(literal) => Some(format!(
            "{value_var} = \"{}\"",
            escape_surql_string(literal)
        )),
        StringValidator::RegexLiteral(format) => Some(matches(value_var, &format.pattern())),
        StringValidator::Length(bound) => match bounds::length(bound) {
            Ok(length) => Some(format!("string::len({value_var}) = {length}")),
            Err(message) => {
                error!("{message}");
                None
            }
        },
        StringValidator::MinLength(length) => Some(format!("string::len({value_var}) >= {length}")),
        StringValidator::MaxLength(length) => Some(format!("string::len({value_var}) <= {length}")),
        StringValidator::NonEmpty => Some(format!("string::len({value_var}) > 0")),
        StringValidator::StartsWith(prefix) => Some(format!(
            "string::starts_with({value_var}, \"{}\")",
            escape_surql_string(prefix)
        )),
        StringValidator::EndsWith(suffix) => Some(format!(
            "string::ends_with({value_var}, \"{}\")",
            escape_surql_string(suffix)
        )),
        StringValidator::Includes(substring) => Some(format!(
            "string::contains({value_var}, \"{}\")",
            escape_surql_string(substring)
        )),
        StringValidator::Trimmed => script("return v.trim() === v;")
            .or_else(|| Some(format!("{value_var} = string::trim({value_var})"))),
        StringValidator::Lowercased => {
            Some(format!("{value_var} = string::lowercase({value_var})"))
        }
        StringValidator::Uppercased => {
            Some(format!("{value_var} = string::uppercase({value_var})"))
        }
        StringValidator::Capitalized => script(CAPITALIZED_JS),
        StringValidator::Uncapitalized => script(UNCAPITALIZED_JS),
        StringValidator::String
        | StringValidator::StringEmbedded(_)
        | StringValidator::Trim
        | StringValidator::Lower
        | StringValidator::Upper
        | StringValidator::Capitalize
        | StringValidator::Normalize
        | StringValidator::NormalizeNFC
        | StringValidator::NormalizeNFD
        | StringValidator::NormalizeNFKC
        | StringValidator::NormalizeNFKD => None,
        StringValidator::IntegerParse
        | StringValidator::NumericParse
        | StringValidator::DateParse
        | StringValidator::DateIsoParse
        | StringValidator::DateEpochParse
        | StringValidator::JsonParse
        | StringValidator::UrlParse => None,
    }
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
            Validator::StringValidator(sv) => {
                match string_assertion(sv, value_var, allow_scripting) {
                    Some(assertion) => assertions.push(assertion),
                    None => trace!("String validator {sv:?} produces no SurrealDB assertion"),
                }
            }

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
                NumberValidator::Finite => {
                    trace!("Number validator Finite produces no assertion (scripting disabled)")
                }
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
                DateValidator::GreaterThanDate(s) => push_date(&mut assertions, value_var, ">", s),
                DateValidator::GreaterThanOrEqualToDate(s) => {
                    push_date(&mut assertions, value_var, ">=", s)
                }
                DateValidator::LessThanDate(s) => push_date(&mut assertions, value_var, "<", s),
                DateValidator::LessThanOrEqualToDate(s) => {
                    push_date(&mut assertions, value_var, "<=", s)
                }
                DateValidator::BetweenDate(a, b) => {
                    match (datetime_literal(a), datetime_literal(b)) {
                        (Ok(la), Ok(lb)) => {
                            assertions.push(format!("{value_var} >= {la} AND {value_var} <= {lb}"))
                        }
                        (Err(message), _) | (_, Err(message)) => {
                            error!("DateValidator::BetweenDate: {message}")
                        }
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
                BigIntValidator::LessThanBigInt(s) => push_int(&mut assertions, value_var, "<", s),
                BigIntValidator::LessThanOrEqualToBigInt(s) => {
                    push_int(&mut assertions, value_var, "<=", s)
                }
                BigIntValidator::BetweenBigInt(a, b) => match (int_literal(a), int_literal(b)) {
                    (Ok(la), Ok(lb)) => {
                        assertions.push(format!("{value_var} >= {la} AND {value_var} <= {lb}"))
                    }
                    (Err(message), _) | (_, Err(message)) => {
                        error!("BigIntValidator::BetweenBigInt: {message}")
                    }
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
                        (Ok(la), Ok(lb)) => {
                            assertions.push(format!("{value_var} >= {la} AND {value_var} <= {lb}"))
                        }
                        (Err(message), _) | (_, Err(message)) => {
                            error!("BigDecimalValidator::BetweenBigDecimal: {message}")
                        }
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
                        (Ok(la), Ok(lb)) => {
                            assertions.push(format!("{value_var} >= {la} AND {value_var} <= {lb}"))
                        }
                        (Err(message), _) | (_, Err(message)) => {
                            error!("DurationValidator::BetweenDuration: {message}")
                        }
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
        Ok(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        Err(message) => error!("{message}"),
    }
}

fn push_int(assertions: &mut Vec<String>, value_var: &str, op: &str, bound: &str) {
    match int_literal(bound) {
        Ok(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        Err(message) => error!("{message}"),
    }
}

fn push_decimal(assertions: &mut Vec<String>, value_var: &str, op: &str, bound: &str) {
    match decimal_literal(bound) {
        Ok(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        Err(message) => error!("{message}"),
    }
}

fn push_duration(assertions: &mut Vec<String>, value_var: &str, op: &str, bound: &str) {
    match duration_literal(bound) {
        Ok(lit) => assertions.push(format!("{value_var} {op} {lit}")),
        Err(message) => error!("{message}"),
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
    fn string_keywords_use_the_shared_patterns() {
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Email)),
            "string::matches($value, \"^[0-9A-Za-z_%+.-]+@[0-9.A-Za-z-]+\\\\.[A-Za-z]{2,}$\")"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Url)),
            "string::is_url($value)"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Digits)),
            "string::matches($value, \"^[0-9]*$\")"
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Integer)),
            "string::matches($value, \"^(?:0|-?[1-9][0-9]*)$\")"
        );
        assert_eq!(
            gen_assert_no_js(Validator::StringValidator(StringValidator::Date)),
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
            "string::matches($value, \"^[a-z]*$\")"
        );
        assert!(
            gen_assert(Validator::StringValidator(
                StringValidator::TrimPreformatted
            ))
            .starts_with("string::matches($value, \"^(?:[^")
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Base64)),
            format!("string::matches($value, \"{}\")", keywords::BASE64)
        );
        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::UuidV4)),
            "string::matches($value, \"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-4[0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$\")"
        );
    }

    #[test]
    fn string_js_variants_gated_on_scripting() {
        let cc = gen_assert(Validator::StringValidator(StringValidator::CreditCard));
        assert!(cc.starts_with("string::matches($value, "));
        assert!(cc.contains(" AND function($value) { const v = arguments[0]; "));
        assert!(cc.contains("sum % 10 === 0"));
        // No newlines — must be single-line for stable round-trip.
        assert!(!cc.contains('\n'));

        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Json)),
            "function($value) { const v = arguments[0]; try { JSON.parse(v); return true; } catch (e) { return false; } }"
        );

        assert_eq!(
            gen_assert(Validator::StringValidator(StringValidator::Regex)),
            "function($value) { const v = arguments[0]; try { new RegExp(v); return true; } catch (e) { return false; } }"
        );

        // Disabling scripting keeps only the native part.
        assert_eq!(
            gen_assert_no_js(Validator::StringValidator(StringValidator::CreditCard)),
            format!("string::matches($value, \"{}\")", keywords::CREDIT_CARD)
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
