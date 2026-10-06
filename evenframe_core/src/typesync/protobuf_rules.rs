//! protoc-gen-validate rules for a protobuf field. Every rule is held once:
//! bounds keep the tightest value, and validators no rule can hold exactly are
//! named so the schema can say so.

use crate::error::{EvenframeError, Result};
use crate::types::FieldType;
use crate::typesync::js_checks::{self, JsCheck, LengthCheck};
use crate::validator::keywords;
use crate::validator::{
    ArrayValidator, DateValidator, NumberValidator, StringValidator, Validator, bounds,
};

/// A field's rules and the validators they cannot hold.
#[derive(Debug, Default)]
pub struct FieldRules {
    /// `(validate.rules).<kind> = {...}`, when any rule applies.
    pub option: Option<String>,
    /// Each validator no rule holds exactly, described.
    pub unenforced: Vec<String>,
    /// Whether a string length rule was written.
    pub counts_length: bool,
}

/// The rule set a field's protobuf type takes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    String,
    Number { rule: &'static str, integer: bool },
    Repeated,
    Timestamp,
    Other,
}

fn kind(field_type: &FieldType, registry: &crate::types::ForeignTypeRegistry) -> Kind {
    match field_type {
        FieldType::Option(inner) => kind(inner, registry),
        FieldType::String | FieldType::Char | FieldType::I128 | FieldType::U128 => Kind::String,
        FieldType::F32 => Kind::Number {
            rule: "float",
            integer: false,
        },
        FieldType::F64 => Kind::Number {
            rule: "double",
            integer: false,
        },
        FieldType::I8 | FieldType::I16 | FieldType::I32 => Kind::Number {
            rule: "int32",
            integer: true,
        },
        FieldType::I64 | FieldType::Isize => Kind::Number {
            rule: "int64",
            integer: true,
        },
        FieldType::U8 | FieldType::U16 | FieldType::U32 => Kind::Number {
            rule: "uint32",
            integer: true,
        },
        FieldType::U64 | FieldType::Usize => Kind::Number {
            rule: "uint64",
            integer: true,
        },
        FieldType::Vec(_) => Kind::Repeated,
        FieldType::Other(name) => match registry.lookup(name) {
            Some(foreign) => {
                let wire = if foreign.protobuf_wire_type.is_empty() {
                    foreign.protobuf.as_str()
                } else {
                    foreign.protobuf_wire_type.as_str()
                };
                match wire {
                    "string" => Kind::String,
                    "float" | "double" => Kind::Number {
                        rule: if wire == "float" { "float" } else { "double" },
                        integer: false,
                    },
                    "int32" | "sint32" | "sfixed32" => Kind::Number {
                        rule: "int32",
                        integer: true,
                    },
                    "int64" | "sint64" | "sfixed64" => Kind::Number {
                        rule: "int64",
                        integer: true,
                    },
                    "uint32" | "fixed32" => Kind::Number {
                        rule: "uint32",
                        integer: true,
                    },
                    "uint64" | "fixed64" => Kind::Number {
                        rule: "uint64",
                        integer: true,
                    },
                    "timestamp" | "google.protobuf.Timestamp" => Kind::Timestamp,
                    _ => Kind::Other,
                }
            }
            None => Kind::Other,
        },
        FieldType::Bool
        | FieldType::Unit
        | FieldType::Tuple(_)
        | FieldType::Struct(_)
        | FieldType::HashMap(..)
        | FieldType::BTreeMap(..)
        | FieldType::RecordLink(_)
        | FieldType::Duration => Kind::Other,
    }
}

/// One side of a range.
#[derive(Clone, Copy)]
struct Bound {
    value: f64,
    strict: bool,
}

/// The tighter of two lower bounds: the higher value, strict on a tie.
fn tighter_lower(current: Option<Bound>, candidate: Bound) -> Bound {
    match current {
        Some(current)
            if current.value > candidate.value
                || (current.value == candidate.value && current.strict) =>
        {
            current
        }
        _ => candidate,
    }
}

/// The tighter of two upper bounds: the lower value, strict on a tie.
fn tighter_upper(current: Option<Bound>, candidate: Bound) -> Bound {
    match current {
        Some(current)
            if current.value < candidate.value
                || (current.value == candidate.value && current.strict) =>
        {
            current
        }
        _ => candidate,
    }
}

#[derive(Default)]
struct Rules {
    // string
    len: Option<usize>,
    min_len: Option<usize>,
    max_len: Option<usize>,
    patterns: Vec<(String, String)>,
    prefix: Option<String>,
    suffix: Option<String>,
    contains: Option<String>,
    constant: Option<String>,
    // number and timestamp
    lower: Option<Bound>,
    upper: Option<Bound>,
    // repeated
    min_items: Option<usize>,
    max_items: Option<usize>,
}

/// The rules for a field named `field` holding `validators`.
pub fn field_rules(
    field: &str,
    validators: &[Validator],
    field_type: &FieldType,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<FieldRules> {
    let kind = kind(field_type, registry);
    let mut rules = Rules::default();
    let mut unenforced = Vec::new();
    let contradiction = |detail: String| {
        EvenframeError::type_sync(format!(
            "the validators on `{field}` contradict each other: {detail}"
        ))
    };

    for validator in validators {
        let held = match (validator, kind) {
            (Validator::StringValidator(string), Kind::String) => {
                string_rule(string, &mut rules, &contradiction)?
            }
            (Validator::NumberValidator(number), Kind::Number { integer, .. }) => {
                number_rule(number, integer, &mut rules)
            }
            (Validator::ArrayValidator(array), Kind::Repeated) => {
                let (minimum, maximum) = match array {
                    ArrayValidator::MinItems(count) => (Some(*count), None),
                    ArrayValidator::MaxItems(count) => (None, Some(*count)),
                    ArrayValidator::ItemsCount(count) => (Some(*count), Some(*count)),
                };
                if let Some(minimum) = minimum {
                    rules.min_items = Some(
                        rules
                            .min_items
                            .map_or(minimum, |current| current.max(minimum)),
                    );
                }
                if let Some(maximum) = maximum {
                    rules.max_items = Some(
                        rules
                            .max_items
                            .map_or(maximum, |current| current.min(maximum)),
                    );
                }
                true
            }
            (Validator::DateValidator(date), Kind::Timestamp) => date_rule(date, &mut rules)?,
            _ => false,
        };
        if !held {
            unenforced.push(validator.describe());
        }
    }

    // One pattern per field: character classes intersect exactly, anything
    // else past the first is named instead.
    if rules.patterns.len() > 1 {
        match intersect_classes(&rules.patterns) {
            Some(Ok(pattern)) => {
                rules.patterns = vec![(pattern, String::new())];
            }
            Some(Err(detail)) => return Err(contradiction(detail)),
            None => {
                for (_, description) in rules.patterns.drain(1..) {
                    unenforced.push(format!(
                        "{description} (protoc-gen-validate holds one pattern per field)"
                    ));
                }
            }
        }
    }

    let (entries, counts_length) = render(kind, &rules, &contradiction)?;
    let option = match (entries.is_empty(), kind) {
        (true, _) => None,
        (false, Kind::String) => Some(format!(
            "(validate.rules).string = {{{}}}",
            entries.join(", ")
        )),
        (false, Kind::Number { rule, .. }) => Some(format!(
            "(validate.rules).{rule} = {{{}}}",
            entries.join(", ")
        )),
        (false, Kind::Repeated) => Some(format!(
            "(validate.rules).repeated = {{{}}}",
            entries.join(", ")
        )),
        (false, Kind::Timestamp) => Some(format!(
            "(validate.rules).timestamp = {{{}}}",
            entries.join(", ")
        )),
        (false, Kind::Other) => None,
    };
    Ok(FieldRules {
        option,
        unenforced,
        counts_length,
    })
}

/// Records a string validator's rule, returning whether one holds it exactly.
fn string_rule(
    validator: &StringValidator,
    rules: &mut Rules,
    contradiction: &impl Fn(String) -> EvenframeError,
) -> Result<bool> {
    match validator {
        // The field's type already guarantees a string.
        StringValidator::String => return Ok(true),
        StringValidator::StartsWith(prefix) => {
            rules.prefix = Some(merge_affix(
                rules.prefix.take(),
                prefix,
                "start",
                contradiction,
                |value, affix| value.starts_with(affix),
            )?);
            return Ok(true);
        }
        StringValidator::EndsWith(suffix) => {
            rules.suffix = Some(merge_affix(
                rules.suffix.take(),
                suffix,
                "end",
                contradiction,
                |value, affix| value.ends_with(affix),
            )?);
            return Ok(true);
        }
        StringValidator::Includes(substring) => {
            return Ok(match rules.contains.take() {
                None => {
                    rules.contains = Some(substring.clone());
                    true
                }
                // Containing the longer one contains the shorter.
                Some(existing) if existing.contains(substring.as_str()) => {
                    rules.contains = Some(existing);
                    true
                }
                Some(existing) if substring.contains(existing.as_str()) => {
                    rules.contains = Some(substring.clone());
                    true
                }
                Some(existing) => {
                    rules.contains = Some(existing);
                    false
                }
            });
        }
        StringValidator::Literal(literal) => {
            if let Some(existing) = &rules.constant
                && existing != literal
            {
                return Err(contradiction(format!(
                    "the value must equal both {existing:?} and {literal:?}"
                )));
            }
            rules.constant = Some(literal.clone());
            return Ok(true);
        }
        StringValidator::Ip => {
            rules.patterns.push((
                format!("{}|{}", re2(keywords::IPV4), re2(keywords::IPV6)),
                validator.description().to_string(),
            ));
            return Ok(true);
        }
        _ => {}
    }
    Ok(match js_checks::string_check(validator)? {
        Some(JsCheck::Length(LengthCheck::Exactly(length))) => {
            if let Some(existing) = rules.len
                && existing != length
            {
                return Err(contradiction(format!(
                    "the length must be both {existing} and {length}"
                )));
            }
            rules.len = Some(length);
            true
        }
        Some(JsCheck::Length(LengthCheck::AtLeast(length))) => {
            rules.min_len = Some(rules.min_len.map_or(length, |current| current.max(length)));
            true
        }
        Some(JsCheck::Length(LengthCheck::AtMost(length))) => {
            rules.max_len = Some(rules.max_len.map_or(length, |current| current.min(length)));
            true
        }
        Some(JsCheck::Pattern { source, flags }) if flags.is_empty() && holds_in_re2(&source) => {
            rules
                .patterns
                .push((re2(&source), validator.description().to_string()));
            true
        }
        Some(JsCheck::Pattern { .. } | JsCheck::Predicate(_)) | None => false,
    })
}

/// Merges two prefixes or suffixes: one must extend the other.
fn merge_affix(
    existing: Option<String>,
    candidate: &str,
    side: &str,
    contradiction: &impl Fn(String) -> EvenframeError,
    extends: impl Fn(&str, &str) -> bool,
) -> Result<String> {
    match existing {
        None => Ok(candidate.to_string()),
        Some(existing) if extends(&existing, candidate) => Ok(existing),
        Some(existing) if extends(candidate, &existing) => Ok(candidate.to_string()),
        Some(existing) => Err(contradiction(format!(
            "the value cannot {side} with both {existing:?} and {candidate:?}"
        ))),
    }
}

fn raise_lower(rules: &mut Rules, value: f64, strict: bool) {
    rules.lower = Some(tighter_lower(rules.lower, Bound { value, strict }));
}

fn lower_upper(rules: &mut Rules, value: f64, strict: bool) {
    rules.upper = Some(tighter_upper(rules.upper, Bound { value, strict }));
}

/// Records a number validator's bounds, returning whether they hold it
/// exactly.
fn number_rule(validator: &NumberValidator, integer: bool, rules: &mut Rules) -> bool {
    match validator {
        NumberValidator::GreaterThan(value) => raise_lower(rules, value.0, true),
        NumberValidator::GreaterThanOrEqualTo(value) => raise_lower(rules, value.0, false),
        NumberValidator::Positive => raise_lower(rules, 0.0, true),
        NumberValidator::NonNegative => raise_lower(rules, 0.0, false),
        NumberValidator::LessThan(value) => lower_upper(rules, value.0, true),
        NumberValidator::LessThanOrEqualTo(value) => lower_upper(rules, value.0, false),
        NumberValidator::Negative => lower_upper(rules, 0.0, true),
        NumberValidator::NonPositive => lower_upper(rules, 0.0, false),
        NumberValidator::Between(start, end) => {
            raise_lower(rules, start.0, false);
            lower_upper(rules, end.0, false);
        }
        // An integer field is always an integer; a float field cannot say so.
        NumberValidator::Int => return integer,
        NumberValidator::Uint8 => {
            raise_lower(rules, 0.0, false);
            lower_upper(rules, 255.0, false);
            return integer;
        }
        NumberValidator::MultipleOf(_) | NumberValidator::Finite | NumberValidator::NonNaN => {
            return false;
        }
    }
    true
}

/// Records a date validator's bounds as Unix seconds.
fn date_rule(validator: &DateValidator, rules: &mut Rules) -> Result<bool> {
    let seconds = |bound: &str| -> Result<f64> {
        bounds::date(bound)
            .map(|date| date.timestamp() as f64)
            .map_err(|error| EvenframeError::type_sync(format!("date bound {bound:?}: {error}")))
    };
    match validator {
        DateValidator::ValidDate => {}
        DateValidator::GreaterThanDate(bound) => raise_lower(rules, seconds(bound)?, true),
        DateValidator::GreaterThanOrEqualToDate(bound) => {
            raise_lower(rules, seconds(bound)?, false)
        }
        DateValidator::LessThanDate(bound) => lower_upper(rules, seconds(bound)?, true),
        DateValidator::LessThanOrEqualToDate(bound) => lower_upper(rules, seconds(bound)?, false),
        DateValidator::BetweenDate(start, end) => {
            raise_lower(rules, seconds(start)?, false);
            lower_upper(rules, seconds(end)?, false);
        }
    }
    Ok(true)
}

/// The merged rules as option entries, and whether a length rule is among
/// them.
fn render(
    kind: Kind,
    rules: &Rules,
    contradiction: &impl Fn(String) -> EvenframeError,
) -> Result<(Vec<String>, bool)> {
    let mut entries = Vec::new();
    let mut counts_length = false;
    match kind {
        Kind::String => {
            let minimum = rules.min_len.unwrap_or(0);
            let maximum = rules.max_len.unwrap_or(usize::MAX);
            if minimum > maximum {
                return Err(contradiction(format!(
                    "the length must be at least {minimum} and at most {maximum}"
                )));
            }
            if let Some(length) = rules.len {
                if length < minimum || length > maximum {
                    return Err(contradiction(format!(
                        "the length must be {length} and between {minimum} and {maximum}"
                    )));
                }
                entries.push(format!("len: {length}"));
                counts_length = true;
            } else {
                if let Some(minimum) = rules.min_len {
                    entries.push(format!("min_len: {minimum}"));
                    counts_length = true;
                }
                if let Some(maximum) = rules.max_len {
                    entries.push(format!("max_len: {maximum}"));
                    counts_length = true;
                }
            }
            if let Some((pattern, _)) = rules.patterns.first() {
                entries.push(format!("pattern: \"{}\"", escape(pattern)));
            }
            if let Some(prefix) = &rules.prefix {
                entries.push(format!("prefix: \"{}\"", escape(prefix)));
            }
            if let Some(suffix) = &rules.suffix {
                entries.push(format!("suffix: \"{}\"", escape(suffix)));
            }
            if let Some(contains) = &rules.contains {
                entries.push(format!("contains: \"{}\"", escape(contains)));
            }
            if let Some(constant) = &rules.constant {
                entries.push(format!("const: \"{}\"", escape(constant)));
            }
        }
        Kind::Number { integer, .. } => {
            entries.extend(range(rules, integer, contradiction, |value| value)?);
        }
        Kind::Timestamp => {
            entries.extend(range(rules, true, contradiction, |value| {
                format!("{{seconds: {value}}}")
            })?);
        }
        Kind::Repeated => {
            if let (Some(minimum), Some(maximum)) = (rules.min_items, rules.max_items)
                && minimum > maximum
            {
                return Err(contradiction(format!(
                    "the list must hold at least {minimum} and at most {maximum} items"
                )));
            }
            if let Some(minimum) = rules.min_items {
                entries.push(format!("min_items: {minimum}"));
            }
            if let Some(maximum) = rules.max_items {
                entries.push(format!("max_items: {maximum}"));
            }
        }
        Kind::Other => {}
    }
    Ok((entries, counts_length))
}

/// A range as `gt`/`gte` and `lt`/`lte` entries. An integer range is held by
/// its inclusive integer ends, which is exact for integer values.
fn range(
    rules: &Rules,
    integer: bool,
    contradiction: &impl Fn(String) -> EvenframeError,
    literal: impl Fn(String) -> String,
) -> Result<Vec<String>> {
    let mut entries = Vec::new();
    if integer {
        let lower = rules.lower.map(|bound| {
            if bound.strict {
                bound.value.floor() + 1.0
            } else {
                bound.value.ceil()
            }
        });
        let upper = rules.upper.map(|bound| {
            if bound.strict {
                bound.value.ceil() - 1.0
            } else {
                bound.value.floor()
            }
        });
        if let (Some(lower), Some(upper)) = (lower, upper)
            && lower > upper
        {
            return Err(contradiction(format!(
                "no integer is at least {lower} and at most {upper}"
            )));
        }
        if let Some(lower) = lower {
            entries.push(format!("gte: {}", literal(format!("{lower}"))));
        }
        if let Some(upper) = upper {
            entries.push(format!("lte: {}", literal(format!("{upper}"))));
        }
        return Ok(entries);
    }
    if let (Some(lower), Some(upper)) = (rules.lower, rules.upper)
        && (lower.value > upper.value
            || (lower.value == upper.value && (lower.strict || upper.strict)))
    {
        return Err(contradiction(format!(
            "no value is above {} and below {}",
            lower.value, upper.value
        )));
    }
    if let Some(lower) = rules.lower {
        let name = if lower.strict { "gt" } else { "gte" };
        entries.push(format!("{name}: {}", literal(format!("{}", lower.value))));
    }
    if let Some(upper) = rules.upper {
        let name = if upper.strict { "lt" } else { "lte" };
        entries.push(format!("{name}: {}", literal(format!("{}", upper.value))));
    }
    Ok(entries)
}

/// Intersects patterns that are all one character class repeated, like
/// `^[0-9A-Za-z]*$`. `None` when any pattern has another shape; an error when
/// the classes share no character but a value must have one.
fn intersect_classes(patterns: &[(String, String)]) -> Option<std::result::Result<String, String>> {
    let mut shared: Option<std::collections::BTreeSet<u8>> = None;
    let mut non_empty = false;
    for (pattern, _) in patterns {
        let (characters, one_or_more) = character_class(pattern)?;
        non_empty |= one_or_more;
        shared = Some(match shared {
            None => characters,
            Some(existing) => existing.intersection(&characters).copied().collect(),
        });
    }
    let shared = shared?;
    if shared.is_empty() {
        return Some(if non_empty {
            Err("their character classes share no character".to_string())
        } else {
            Ok("^$".to_string())
        });
    }
    let quantifier = if non_empty { '+' } else { '*' };
    Some(Ok(format!("^[{}]{quantifier}$", class_ranges(&shared))))
}

/// The ASCII characters of a `^[...]*$` or `^[...]+$` pattern, and whether it
/// needs at least one.
fn character_class(pattern: &str) -> Option<(std::collections::BTreeSet<u8>, bool)> {
    let body = pattern.strip_prefix("^[")?;
    let (class, one_or_more) = match body.strip_suffix("]*$") {
        Some(class) => (class, false),
        None => (body.strip_suffix("]+$")?, true),
    };
    let bytes = class.as_bytes();
    let mut characters = std::collections::BTreeSet::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = bytes[index];
        if !start.is_ascii_alphanumeric() {
            return None;
        }
        if bytes.get(index + 1) == Some(&b'-') && index + 2 < bytes.len() {
            let end = bytes[index + 2];
            if !end.is_ascii_alphanumeric() || end < start {
                return None;
            }
            characters.extend(start..=end);
            index += 3;
        } else {
            characters.insert(start);
            index += 1;
        }
    }
    Some((characters, one_or_more))
}

/// A set of ASCII alphanumerics as class ranges, `0-9a-f`.
fn class_ranges(characters: &std::collections::BTreeSet<u8>) -> String {
    let mut output = String::new();
    let mut run: Option<(u8, u8)> = None;
    let flush = |run: (u8, u8), output: &mut String| {
        output.push(char::from(run.0));
        if run.1 > run.0 {
            if run.1 > run.0 + 1 {
                output.push('-');
            }
            output.push(char::from(run.1));
        }
    };
    for &character in characters {
        run = match run {
            Some((start, end)) if character == end + 1 => Some((start, character)),
            Some(previous) => {
                flush(previous, &mut output);
                Some((character, character))
            }
            None => Some((character, character)),
        };
    }
    if let Some(run) = run {
        flush(run, &mut output);
    }
    output
}

/// Whether RE2, which protoc-gen-validate's checkers use, can run `source`:
/// it has no lookaround and no backreferences.
fn holds_in_re2(source: &str) -> bool {
    !["(?=", "(?!", "(?<=", "(?<!"]
        .iter()
        .any(|lookaround| source.contains(lookaround))
        && !source
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b'\\' && pair[1].is_ascii_digit() && pair[1] != b'0')
}

/// A regex source in RE2 syntax: `\uXXXX` escapes become `\x{XXXX}`.
fn re2(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek() == Some(&'u') {
            characters.next();
            let digits: String = characters.by_ref().take(4).collect();
            output.push_str(&format!("\\x{{{digits}}}"));
        } else {
            output.push(character);
            if character == '\\'
                && let Some(escaped) = characters.next()
            {
                output.push(escaped);
            }
        }
    }
    output
}

/// Escapes text for a protobuf string literal.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::{
        ArrayValidator, FieldRules, FieldType, NumberValidator, Result, StringValidator, Validator,
        field_rules,
    };
    use crate::types::ForeignTypeRegistry;
    use ordered_float::OrderedFloat;

    fn rules(validators: Vec<Validator>, field_type: FieldType) -> Result<FieldRules> {
        field_rules(
            "field",
            &validators,
            &field_type,
            &ForeignTypeRegistry::default(),
        )
    }

    fn string(validator: StringValidator) -> Validator {
        Validator::StringValidator(validator)
    }

    fn number(validator: NumberValidator) -> Validator {
        Validator::NumberValidator(validator)
    }

    #[test]
    fn duplicate_bounds_merge_to_the_tightest() {
        let merged = rules(
            vec![
                number(NumberValidator::Positive),
                number(NumberValidator::GreaterThanOrEqualTo(OrderedFloat(0.001))),
                number(NumberValidator::LessThanOrEqualTo(OrderedFloat(999.999))),
                number(NumberValidator::LessThan(OrderedFloat(1000.0))),
                number(NumberValidator::GreaterThan(OrderedFloat(0.0))),
            ],
            FieldType::F32,
        )
        .unwrap();
        assert_eq!(
            merged.option.as_deref(),
            Some("(validate.rules).float = {gte: 0.001, lte: 999.999}")
        );
        let equal = rules(
            vec![
                number(NumberValidator::GreaterThanOrEqualTo(OrderedFloat(0.0))),
                number(NumberValidator::GreaterThan(OrderedFloat(0.0))),
            ],
            FieldType::F64,
        )
        .unwrap();
        assert_eq!(
            equal.option.as_deref(),
            Some("(validate.rules).double = {gt: 0}")
        );
    }

    #[test]
    fn integer_bounds_become_inclusive_integers() {
        let merged = rules(
            vec![
                number(NumberValidator::GreaterThan(OrderedFloat(0.5))),
                number(NumberValidator::LessThan(OrderedFloat(9.5))),
                number(NumberValidator::Uint8),
                number(NumberValidator::Int),
            ],
            FieldType::U8,
        )
        .unwrap();
        assert_eq!(
            merged.option.as_deref(),
            Some("(validate.rules).uint32 = {gte: 1, lte: 9}")
        );
        assert!(merged.unenforced.is_empty(), "{:?}", merged.unenforced);
    }

    #[test]
    fn contradictory_validators_are_rejected() {
        let error = rules(
            vec![
                number(NumberValidator::GreaterThan(OrderedFloat(5.0))),
                number(NumberValidator::LessThan(OrderedFloat(6.0))),
            ],
            FieldType::I32,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("`field`") && error.contains("no integer"),
            "{error}"
        );
        assert!(
            rules(
                vec![
                    string(StringValidator::MinLength(5)),
                    string(StringValidator::MaxLength(2))
                ],
                FieldType::String
            )
            .is_err()
        );
        assert!(
            rules(
                vec![
                    string(StringValidator::StartsWith("ab".into())),
                    string(StringValidator::StartsWith("ac".into()))
                ],
                FieldType::String
            )
            .is_err()
        );
    }

    #[test]
    fn lengths_merge_and_are_counted() {
        let merged = rules(
            vec![
                string(StringValidator::NonEmpty),
                string(StringValidator::MinLength(8)),
                string(StringValidator::MaxLength(128)),
            ],
            FieldType::String,
        )
        .unwrap();
        assert_eq!(
            merged.option.as_deref(),
            Some("(validate.rules).string = {min_len: 8, max_len: 128}")
        );
        assert!(merged.counts_length);
    }

    #[test]
    fn character_classes_intersect_into_one_pattern() {
        let merged = rules(
            vec![
                string(StringValidator::Alphanumeric),
                string(StringValidator::LowerPreformatted),
            ],
            FieldType::String,
        )
        .unwrap();
        assert_eq!(
            merged.option.as_deref(),
            Some("(validate.rules).string = {pattern: \"^[a-z]*$\"}")
        );
        let hex_digits = rules(
            vec![
                string(StringValidator::Hex),
                string(StringValidator::Digits),
            ],
            FieldType::String,
        )
        .unwrap();
        assert_eq!(
            hex_digits.option.as_deref(),
            Some("(validate.rules).string = {pattern: \"^[0-9]+$\"}")
        );
        assert!(
            rules(
                vec![
                    string(StringValidator::Hex),
                    string(StringValidator::UpperPreformatted)
                ],
                FieldType::String
            )
            .is_ok()
        );
    }

    #[test]
    fn a_second_pattern_that_cannot_intersect_is_named() {
        let merged = rules(
            vec![
                string(StringValidator::Uuid),
                string(StringValidator::Digits),
            ],
            FieldType::String,
        )
        .unwrap();
        assert_eq!(merged.unenforced.len(), 1, "{:?}", merged.unenforced);
        assert!(merged.unenforced[0].contains("one pattern per field"));
    }

    #[test]
    fn checks_rules_cannot_hold_are_named() {
        let merged = rules(
            vec![
                string(StringValidator::Lowercased),
                string(StringValidator::Json),
                string(StringValidator::DateIso),
                number(NumberValidator::Positive),
            ],
            FieldType::String,
        )
        .unwrap();
        assert_eq!(merged.option, None);
        assert_eq!(merged.unenforced.len(), 4, "{:?}", merged.unenforced);
    }

    #[test]
    fn keyword_patterns_are_written_for_re2() {
        let capitalized = rules(
            vec![string(StringValidator::CapitalizePreformatted)],
            FieldType::String,
        )
        .unwrap();
        let option = capitalized.option.unwrap_or_default();
        assert!(option.contains("\\\\x{2028}"), "{option}");
        assert!(!option.contains("\\\\u2028"), "{option}");
    }

    #[test]
    fn list_counts_merge() {
        let merged = rules(
            vec![
                Validator::ArrayValidator(ArrayValidator::MinItems(1)),
                Validator::ArrayValidator(ArrayValidator::ItemsCount(3)),
            ],
            FieldType::Vec(Box::new(FieldType::String)),
        )
        .unwrap();
        assert_eq!(
            merged.option.as_deref(),
            Some("(validate.rules).repeated = {min_items: 3, max_items: 3}")
        );
    }
}
