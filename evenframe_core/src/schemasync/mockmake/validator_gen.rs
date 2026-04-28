//! Validator-aware mock value generation.
//!
//! When a `StructField` carries `#[validators(...)]`, the constraints are
//! mirrored into SurrealDB `DEFINE FIELD ... ASSERT ...` clauses. A naive
//! random value would frequently violate those asserts; this module produces
//! a SurrealQL literal that satisfies the validator set by construction
//! whenever it can solve the constraints in closed form, and returns `None`
//! otherwise so the caller can fall back to the default generator + a
//! retry-and-check loop.
//!
//! Wiring lives in `field_value.rs`: after the WASM-plugin tier, the
//! coordinated-values check, and the explicit `Format` check, but before the
//! type-default `match`.

use crate::schemasync::mockmake::{Mockmaker, regex_val_gen::RegexValGen};
use crate::types::FieldType;
use crate::validator::{
    ArrayValidator, MockValue, NumberValidator, StringValidator, Validator,
};
use rand::{RngExt, rngs::ThreadRng};

/// Cap on how many times we'll regenerate a string when it doesn't satisfy
/// every validator on the first try (e.g. a regex-driven candidate that
/// happens to fall outside a length bound).
const STRING_GEN_ATTEMPTS: usize = 16;

/// Produce a SurrealQL literal that satisfies every validator in
/// `validators` for a field of type `field_type`, or `None` if the constraints
/// can't be solved in closed form.
pub fn generate_with_validators(
    field_type: &FieldType,
    validators: &[Validator],
    rng: &mut ThreadRng,
) -> Option<String> {
    if validators.is_empty() {
        return None;
    }
    match field_type {
        FieldType::String => generate_string(validators, rng),
        FieldType::F32 | FieldType::F64 => generate_float(validators, rng),
        FieldType::I8
        | FieldType::I16
        | FieldType::I32
        | FieldType::I64
        | FieldType::I128
        | FieldType::Isize
        | FieldType::U8
        | FieldType::U16
        | FieldType::U32
        | FieldType::U64
        | FieldType::U128
        | FieldType::Usize => generate_integer(field_type, validators, rng),
        // Containers, options, records, and foreign types are handled by the
        // existing recursion in field_value.rs. ArrayValidator on a Vec field
        // is honoured separately via `array_count_range`.
        _ => None,
    }
}

/// Pick the element-count for a `Vec<_>` field given the field's array
/// validators. `default_lo`/`default_hi` mirror the existing
/// `rng.random_range(2..10)` defaults from `field_value.rs`.
///
/// Always returns `lo <= hi` (collapses to `lo` if validators contradict the
/// defaults). Caller still drives the RNG.
pub fn array_count_range(
    validators: &[Validator],
    default_lo: usize,
    default_hi: usize,
) -> (usize, usize) {
    let mut lo = default_lo;
    let mut hi = default_hi;
    for v in validators {
        if let Validator::ArrayValidator(av) = v {
            match av {
                ArrayValidator::MinItems(n) => lo = lo.max(*n),
                ArrayValidator::MaxItems(n) => hi = hi.min(*n),
                ArrayValidator::ItemsCount(n) => {
                    lo = *n;
                    hi = *n;
                }
            }
        }
    }
    if lo > hi {
        hi = lo;
    }
    (lo, hi)
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StringConstraints {
    /// Bounds on chars().count(). `None` means unconstrained.
    min_len: Option<usize>,
    max_len: Option<usize>,
    /// `Length(n)` collapses both bounds.
    exact_len: Option<usize>,
    /// Substrings the value must include / start with / end with.
    starts_with: Vec<String>,
    ends_with: Vec<String>,
    includes: Vec<String>,
    /// Apply lower/upper/trim/capitalize transformations after generation.
    to_lower: bool,
    to_upper: bool,
    to_trim: bool,
    to_capitalize: bool,
    /// True when a `Literal(s)` is present — value must equal exactly `s`.
    literal: Option<String>,
    /// Exactly one "shape" picked from the validator set. The first one we
    /// see wins; conflicting shapes (`Email` + `Uuid`) collapse to `None`
    /// and the value is generated through the regex/random fallback path.
    shape: Option<StringShape>,
    /// Regex pattern (from `RegexLiteral(format)`).
    regex_format: Option<crate::schemasync::mockmake::format::Format>,
}

#[derive(Clone, Copy)]
enum StringShape {
    Email,
    Uuid,
    Url,
    Ip,
    IpV4,
    IpV6,
    Hex,
    Alpha,
    Alphanumeric,
    Digits,
    Numeric,
    Integer,
    CreditCard,
    Semver,
    DateIso,
    DateYmd,
    DateEpoch,
}

fn collect_string_constraints(validators: &[Validator]) -> StringConstraints {
    let mut c = StringConstraints::default();
    for v in validators {
        let Validator::StringValidator(sv) = v else {
            continue;
        };
        match sv {
            StringValidator::MinLength(n) => {
                c.min_len = Some(c.min_len.map_or(*n, |m| m.max(*n)));
            }
            StringValidator::MaxLength(n) => {
                c.max_len = Some(c.max_len.map_or(*n, |m| m.min(*n)));
            }
            StringValidator::Length(s) => {
                if let Ok(n) = s.parse::<usize>() {
                    c.exact_len = Some(n);
                }
            }
            StringValidator::NonEmpty => {
                c.min_len = Some(c.min_len.map_or(1, |m| m.max(1)));
            }
            StringValidator::StartsWith(s) => c.starts_with.push(s.clone()),
            StringValidator::EndsWith(s) => c.ends_with.push(s.clone()),
            StringValidator::Includes(s) => c.includes.push(s.clone()),
            StringValidator::Lower
            | StringValidator::LowerPreformatted
            | StringValidator::Lowercased => c.to_lower = true,
            StringValidator::Upper
            | StringValidator::UpperPreformatted
            | StringValidator::Uppercased => c.to_upper = true,
            StringValidator::Trim | StringValidator::TrimPreformatted | StringValidator::Trimmed => {
                c.to_trim = true
            }
            StringValidator::Capitalize
            | StringValidator::CapitalizePreformatted
            | StringValidator::Capitalized => c.to_capitalize = true,
            StringValidator::Literal(s) => c.literal = Some(s.clone()),
            StringValidator::RegexLiteral(fmt) => c.regex_format = Some(fmt.clone()),
            StringValidator::Email => c.shape = c.shape.or(Some(StringShape::Email)),
            StringValidator::Uuid
            | StringValidator::UuidV1
            | StringValidator::UuidV2
            | StringValidator::UuidV3
            | StringValidator::UuidV4
            | StringValidator::UuidV5
            | StringValidator::UuidV6
            | StringValidator::UuidV7
            | StringValidator::UuidV8 => c.shape = c.shape.or(Some(StringShape::Uuid)),
            StringValidator::Url => c.shape = c.shape.or(Some(StringShape::Url)),
            StringValidator::Ip => c.shape = c.shape.or(Some(StringShape::Ip)),
            StringValidator::IpV4 => c.shape = c.shape.or(Some(StringShape::IpV4)),
            StringValidator::IpV6 => c.shape = c.shape.or(Some(StringShape::IpV6)),
            StringValidator::Hex => c.shape = c.shape.or(Some(StringShape::Hex)),
            StringValidator::Alpha => c.shape = c.shape.or(Some(StringShape::Alpha)),
            StringValidator::Alphanumeric => {
                c.shape = c.shape.or(Some(StringShape::Alphanumeric))
            }
            StringValidator::Digits => c.shape = c.shape.or(Some(StringShape::Digits)),
            StringValidator::Numeric | StringValidator::NumericParse => {
                c.shape = c.shape.or(Some(StringShape::Numeric))
            }
            StringValidator::Integer | StringValidator::IntegerParse => {
                c.shape = c.shape.or(Some(StringShape::Integer))
            }
            StringValidator::CreditCard => c.shape = c.shape.or(Some(StringShape::CreditCard)),
            StringValidator::Semver => c.shape = c.shape.or(Some(StringShape::Semver)),
            StringValidator::DateIso | StringValidator::DateIsoParse => {
                c.shape = c.shape.or(Some(StringShape::DateIso))
            }
            StringValidator::Date | StringValidator::DateParse => {
                c.shape = c.shape.or(Some(StringShape::DateYmd))
            }
            StringValidator::DateEpoch | StringValidator::DateEpochParse => {
                c.shape = c.shape.or(Some(StringShape::DateEpoch))
            }
            // Variants we either can't drive or that don't influence
            // generation (StringEmbedded, Regex with no payload, base64,
            // unicode normalization morphs, Uncapitalized).
            _ => {}
        }
    }
    c
}

fn generate_string(validators: &[Validator], rng: &mut ThreadRng) -> Option<String> {
    let c = collect_string_constraints(validators);

    // A literal pin overrides everything.
    if let Some(ref lit) = c.literal {
        let mut quoted = String::with_capacity(lit.len() + 2);
        quoted.push('\'');
        quoted.push_str(&escape_surql_string(lit));
        quoted.push('\'');
        return Some(quoted);
    }

    for _ in 0..STRING_GEN_ATTEMPTS {
        let Some(candidate) = build_string_candidate(&c, rng) else {
            return None;
        };
        if validators
            .iter()
            .all(|v| v.matches(&MockValue::Str(&candidate)))
        {
            let mut quoted = String::with_capacity(candidate.len() + 2);
            quoted.push('\'');
            quoted.push_str(&escape_surql_string(&candidate));
            quoted.push('\'');
            return Some(quoted);
        }
    }
    None
}

fn build_string_candidate(c: &StringConstraints, rng: &mut ThreadRng) -> Option<String> {
    let target_len = pick_target_len(c, rng);

    // 1. Seed value from the shape/regex/length pipeline.
    let mut s = if let Some(fmt) = &c.regex_format {
        let mut maker = RegexValGen::new();
        maker
            .generate(fmt.clone().into_regex().as_str())
            .ok()?
    } else if let Some(shape) = c.shape {
        gen_shape(shape, target_len, rng)?
    } else {
        Mockmaker::random_string(target_len.unwrap_or(8))
    };

    // 2. Splice in starts_with / ends_with / includes literals.
    for prefix in &c.starts_with {
        if !s.starts_with(prefix.as_str()) {
            s = format!("{}{}", prefix, s);
        }
    }
    for suffix in &c.ends_with {
        if !s.ends_with(suffix.as_str()) {
            s = format!("{}{}", s, suffix);
        }
    }
    for needle in &c.includes {
        if !s.contains(needle.as_str()) {
            // Insert in the middle to avoid colliding with starts_with/ends_with.
            let mid = s.chars().count() / 2;
            let split: usize = s
                .char_indices()
                .nth(mid)
                .map(|(i, _)| i)
                .unwrap_or(s.len());
            s.insert_str(split, needle);
        }
    }

    // 3. Re-clamp to length bounds. We may have grown past max_len after
    // splicing; truncate by chars (not bytes) to stay valid UTF-8.
    if let Some(max) = effective_max_len(c) {
        if s.chars().count() > max {
            s = s.chars().take(max).collect();
        }
    }
    if let Some(min) = effective_min_len(c) {
        let cur = s.chars().count();
        if cur < min {
            // Pad with random alphanumerics — safe for most shape regexes
            // (alpha/alphanumeric/random/regex-driven won't be invalidated
            // by appended ASCII). If the shape is structural (uuid, email,
            // semver), the post-validation `matches` loop will catch the
            // mismatch and trigger another attempt.
            s.push_str(&Mockmaker::random_string(min - cur));
        }
    }

    // 4. Apply transformations (order matters: trim before case so trailing
    // whitespace doesn't survive; capitalize last because it depends on
    // the first character being a letter).
    if c.to_trim {
        s = s.trim().to_string();
    }
    if c.to_lower {
        s = s.to_lowercase();
    }
    if c.to_upper {
        s = s.to_uppercase();
    }
    if c.to_capitalize {
        let mut chars = s.chars();
        s = match chars.next() {
            None => String::new(),
            Some(first) => {
                let head: String = first.to_uppercase().collect();
                let tail: String = chars.as_str().to_lowercase();
                head + tail.as_str()
            }
        };
    }

    Some(s)
}

fn pick_target_len(c: &StringConstraints, rng: &mut ThreadRng) -> Option<usize> {
    if let Some(n) = c.exact_len {
        return Some(n);
    }
    let lo = effective_min_len(c).unwrap_or(0);
    let hi = effective_max_len(c).unwrap_or(lo.max(16));
    if hi < lo {
        return Some(lo);
    }
    if hi == lo {
        return Some(lo);
    }
    Some(rng.random_range(lo..=hi))
}

fn effective_min_len(c: &StringConstraints) -> Option<usize> {
    match (c.exact_len, c.min_len) {
        (Some(n), _) => Some(n),
        (None, m) => m,
    }
}

fn effective_max_len(c: &StringConstraints) -> Option<usize> {
    match (c.exact_len, c.max_len) {
        (Some(n), _) => Some(n),
        (None, m) => m,
    }
}

fn gen_shape(shape: StringShape, target_len: Option<usize>, rng: &mut ThreadRng) -> Option<String> {
    use crate::schemasync::mockmake::format::Format;
    let format_via = |f: Format| -> Option<String> {
        let mut maker = RegexValGen::new();
        maker.generate(f.into_regex().as_str()).ok()
    };
    match shape {
        StringShape::Email => format_via(Format::Email),
        StringShape::Uuid => format_via(Format::Uuid),
        StringShape::Url => format_via(Format::Url("example.com".to_string())),
        StringShape::Ip | StringShape::IpV4 => format_via(Format::IpAddress),
        StringShape::IpV6 => {
            // No Format::IpV6 today; build one inline.
            let mut parts = Vec::with_capacity(8);
            for _ in 0..8 {
                parts.push(format!("{:x}", rng.random_range(0u32..=0xFFFF)));
            }
            Some(parts.join(":"))
        }
        StringShape::Hex => {
            let len = target_len.unwrap_or(16).max(1);
            format_via(Format::HexString(len))
        }
        StringShape::Alpha => Some(random_from_alphabet(target_len.unwrap_or(8), ALPHA, rng)),
        StringShape::Alphanumeric => Some(random_from_alphabet(
            target_len.unwrap_or(8),
            ALPHANUM,
            rng,
        )),
        StringShape::Digits => Some(random_from_alphabet(target_len.unwrap_or(8), DIGITS, rng)),
        StringShape::Numeric => {
            let int_part: u64 = rng.random_range(0u64..=99_999);
            let frac: u32 = rng.random_range(0u32..=999);
            Some(format!("{}.{:03}", int_part, frac))
        }
        StringShape::Integer => Some(format!("{}", rng.random_range(-1_000_000i64..=1_000_000))),
        StringShape::CreditCard => format_via(Format::CreditCardNumber),
        StringShape::Semver => Some(format!(
            "{}.{}.{}",
            rng.random_range(0u32..=20),
            rng.random_range(0u32..=20),
            rng.random_range(0u32..=99)
        )),
        StringShape::DateIso => format_via(Format::DateTime),
        StringShape::DateYmd => format_via(Format::Date),
        StringShape::DateEpoch => Some(format!("{}", rng.random_range(0i64..=2_000_000_000))),
    }
}

const ALPHA: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const ALPHANUM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const DIGITS: &str = "0123456789";

fn random_from_alphabet(len: usize, alphabet: &str, rng: &mut ThreadRng) -> String {
    let chars: Vec<char> = alphabet.chars().collect();
    (0..len)
        .map(|_| chars[rng.random_range(0..chars.len())])
        .collect()
}

/// Escape a string so it can sit inside `'...'` in SurrealQL. Only `\` and
/// `'` need handling for that quoting style.
fn escape_surql_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct NumericRange {
    /// Inclusive lower bound, in f64. `f64::NEG_INFINITY` means unconstrained.
    lo: f64,
    /// Inclusive upper bound, in f64. `f64::INFINITY` means unconstrained.
    hi: f64,
    /// `MultipleOf(d)`. Zero means no constraint.
    multiple_of: f64,
    require_int: bool,
    require_uint8: bool,
}

impl NumericRange {
    fn new() -> Self {
        Self {
            lo: f64::NEG_INFINITY,
            hi: f64::INFINITY,
            multiple_of: 0.0,
            require_int: false,
            require_uint8: false,
        }
    }
}

fn collect_numeric_range(validators: &[Validator]) -> NumericRange {
    let mut r = NumericRange::new();
    for v in validators {
        let Validator::NumberValidator(nv) = v else {
            continue;
        };
        match nv {
            // Strict bounds nudge by f64::EPSILON so a uniform sample that
            // lands exactly on the boundary still passes the validator.
            NumberValidator::GreaterThan(min) => r.lo = r.lo.max(min.0 + f64::EPSILON.max(1.0)),
            NumberValidator::GreaterThanOrEqualTo(min) => r.lo = r.lo.max(min.0),
            NumberValidator::LessThan(max) => r.hi = r.hi.min(max.0 - f64::EPSILON.max(1.0)),
            NumberValidator::LessThanOrEqualTo(max) => r.hi = r.hi.min(max.0),
            NumberValidator::Between(lo, hi) => {
                r.lo = r.lo.max(lo.0);
                r.hi = r.hi.min(hi.0);
            }
            NumberValidator::Positive => r.lo = r.lo.max(1.0),
            NumberValidator::NonNegative => r.lo = r.lo.max(0.0),
            NumberValidator::Negative => r.hi = r.hi.min(-1.0),
            NumberValidator::NonPositive => r.hi = r.hi.min(0.0),
            NumberValidator::Int => r.require_int = true,
            NumberValidator::Uint8 => {
                r.require_uint8 = true;
                r.lo = r.lo.max(0.0);
                r.hi = r.hi.min(255.0);
                r.require_int = true;
            }
            NumberValidator::MultipleOf(d) => r.multiple_of = d.0,
            NumberValidator::NonNaN | NumberValidator::Finite => {
                // Random sampling inside f64 range never produces NaN/Infinity.
            }
        }
    }
    r
}

fn integer_field_default_range(field_type: &FieldType) -> (f64, f64) {
    // Mirror the existing 0..100 default in field_value.rs but widen for
    // signed types so Negative/NonPositive validators can still sample.
    match field_type {
        FieldType::U8 => (0.0, 255.0),
        FieldType::U16 => (0.0, 65_535.0),
        FieldType::U32 | FieldType::U64 | FieldType::U128 | FieldType::Usize => (0.0, 1_000_000.0),
        FieldType::I8 => (-128.0, 127.0),
        FieldType::I16 => (-32_768.0, 32_767.0),
        FieldType::I32 | FieldType::I64 | FieldType::I128 | FieldType::Isize => {
            (-1_000_000.0, 1_000_000.0)
        }
        _ => (0.0, 100.0),
    }
}

fn generate_integer(
    field_type: &FieldType,
    validators: &[Validator],
    rng: &mut ThreadRng,
) -> Option<String> {
    let mut r = collect_numeric_range(validators);
    r.require_int = true;
    let (default_lo, default_hi) = integer_field_default_range(field_type);
    if r.lo == f64::NEG_INFINITY {
        r.lo = default_lo;
    }
    if r.hi == f64::INFINITY {
        r.hi = default_hi;
    }
    let value = sample_numeric(&r, rng)?;
    let int_value = value as i128;
    // Verify against all validators with the integer view.
    if !validators
        .iter()
        .all(|v| v.matches(&MockValue::Num(int_value as f64)))
    {
        return None;
    }
    Some(format!("{}", int_value))
}

fn generate_float(validators: &[Validator], rng: &mut ThreadRng) -> Option<String> {
    let mut r = collect_numeric_range(validators);
    if r.lo == f64::NEG_INFINITY {
        r.lo = 0.0;
    }
    if r.hi == f64::INFINITY {
        r.hi = 100.0;
    }
    let value = sample_numeric(&r, rng)?;
    // Round to the same precision used in the emitted literal so the matches
    // check is performed on the value the database actually sees.
    let rounded = (value * 100.0).round() / 100.0;
    if !validators
        .iter()
        .all(|v| v.matches(&MockValue::Num(rounded)))
    {
        return None;
    }
    Some(format!("{:.2}f", rounded))
}

fn sample_numeric(r: &NumericRange, rng: &mut ThreadRng) -> Option<f64> {
    if r.lo > r.hi {
        return None;
    }
    let raw = if r.lo == r.hi {
        r.lo
    } else if r.require_int || r.require_uint8 {
        let lo = r.lo.ceil() as i128;
        let hi = r.hi.floor() as i128;
        if lo > hi {
            return None;
        }
        if lo == hi {
            lo as f64
        } else {
            // rand 0.10's random_range over i128 isn't directly available;
            // sample via i64 when possible, else fall back to f64.
            if lo >= i64::MIN as i128 && hi <= i64::MAX as i128 {
                rng.random_range(lo as i64..=hi as i64) as f64
            } else {
                let span = (r.hi - r.lo).max(0.0);
                (r.lo + rng.random_range(0.0..=span)).round()
            }
        }
    } else {
        rng.random_range(r.lo..=r.hi)
    };

    if r.multiple_of != 0.0 {
        // Snap to the nearest multiple inside the range. If the snapped
        // value falls outside, walk one step inward.
        let m = r.multiple_of;
        let mut snapped = (raw / m).round() * m;
        if snapped < r.lo {
            snapped = (r.lo / m).ceil() * m;
        }
        if snapped > r.hi {
            snapped = (r.hi / m).floor() * m;
        }
        if snapped < r.lo || snapped > r.hi {
            return None;
        }
        return Some(snapped);
    }
    Some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;

    /// Strip the surrounding `'…'` quoting so we can test the underlying value.
    fn unquote(s: &str) -> &str {
        s.strip_prefix('\'')
            .and_then(|s| s.strip_suffix('\''))
            .unwrap_or(s)
    }

    #[test]
    fn string_min_max_length_satisfied() {
        let validators = vec![
            Validator::StringValidator(StringValidator::MinLength(8)),
            Validator::StringValidator(StringValidator::MaxLength(12)),
        ];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let lit = generate_with_validators(&FieldType::String, &validators, &mut rng)
                .expect("should produce a value");
            let inner = unquote(&lit);
            let len = inner.chars().count();
            assert!(
                (8..=12).contains(&len),
                "got {} (len {}) outside [8,12]",
                inner,
                len
            );
            for v in &validators {
                assert!(v.matches(&MockValue::Str(inner)));
            }
        }
    }

    #[test]
    fn string_email_shape_satisfies_email_validator() {
        let validators = vec![Validator::StringValidator(StringValidator::Email)];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let lit = generate_with_validators(&FieldType::String, &validators, &mut rng)
                .expect("should produce an email");
            let inner = unquote(&lit);
            assert!(
                validators[0].matches(&MockValue::Str(inner)),
                "{} not a valid email",
                inner
            );
        }
    }

    #[test]
    fn string_starts_with_prefix() {
        let validators = vec![
            Validator::StringValidator(StringValidator::StartsWith("ID-".into())),
            Validator::StringValidator(StringValidator::MinLength(6)),
        ];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let lit = generate_with_validators(&FieldType::String, &validators, &mut rng)
                .expect("should produce a value");
            let inner = unquote(&lit);
            assert!(inner.starts_with("ID-"), "missing prefix: {}", inner);
            for v in &validators {
                assert!(v.matches(&MockValue::Str(inner)));
            }
        }
    }

    #[test]
    fn string_lowercase_transformation() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Lowercased),
            Validator::StringValidator(StringValidator::MinLength(4)),
        ];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let lit = generate_with_validators(&FieldType::String, &validators, &mut rng)
                .expect("should produce a value");
            let inner = unquote(&lit);
            for v in &validators {
                assert!(v.matches(&MockValue::Str(inner)));
            }
        }
    }

    #[test]
    fn integer_between_satisfied() {
        let validators = vec![Validator::NumberValidator(NumberValidator::Between(
            OrderedFloat(10.0),
            OrderedFloat(20.0),
        ))];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let lit = generate_with_validators(&FieldType::I32, &validators, &mut rng)
                .expect("should produce an integer");
            let n: f64 = lit.parse().expect("integer literal");
            assert!((10.0..=20.0).contains(&n), "{} outside [10,20]", n);
        }
    }

    #[test]
    fn integer_positive_and_multiple_of() {
        let validators = vec![
            Validator::NumberValidator(NumberValidator::Positive),
            Validator::NumberValidator(NumberValidator::MultipleOf(OrderedFloat(5.0))),
        ];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let lit = generate_with_validators(&FieldType::I64, &validators, &mut rng)
                .expect("should produce an integer");
            let n: f64 = lit.parse().expect("integer literal");
            assert!(n > 0.0, "got non-positive {}", n);
            assert!(
                (n % 5.0).abs() < f64::EPSILON,
                "{} not a multiple of 5",
                n
            );
        }
    }

    #[test]
    fn float_greater_than_or_equal_to() {
        let validators = vec![Validator::NumberValidator(
            NumberValidator::GreaterThanOrEqualTo(OrderedFloat(50.0)),
        )];
        let mut rng = rand::rng();
        for _ in 0..50 {
            let lit = generate_with_validators(&FieldType::F64, &validators, &mut rng)
                .expect("should produce a float");
            // strip trailing 'f'
            let body = lit.strip_suffix('f').unwrap_or(&lit);
            let n: f64 = body.parse().expect("float literal");
            assert!(n >= 50.0, "{} below 50.0", n);
        }
    }

    #[test]
    fn array_count_range_min_items() {
        let v = vec![Validator::ArrayValidator(ArrayValidator::MinItems(7))];
        let (lo, hi) = array_count_range(&v, 2, 9);
        assert!(lo >= 7);
        assert!(hi >= lo);
    }

    #[test]
    fn array_count_range_exact() {
        let v = vec![Validator::ArrayValidator(ArrayValidator::ItemsCount(4))];
        let (lo, hi) = array_count_range(&v, 2, 9);
        assert_eq!(lo, 4);
        assert_eq!(hi, 4);
    }

    #[test]
    fn empty_validators_yields_none() {
        let mut rng = rand::rng();
        assert!(generate_with_validators(&FieldType::String, &[], &mut rng).is_none());
        assert!(generate_with_validators(&FieldType::I32, &[], &mut rng).is_none());
    }

    #[test]
    fn unsupported_type_yields_none() {
        let validators = vec![Validator::NumberValidator(NumberValidator::Positive)];
        let mut rng = rand::rng();
        // Bool is not handled by validator_gen — caller falls back to default.
        assert!(generate_with_validators(&FieldType::Bool, &validators, &mut rng).is_none());
    }
}

