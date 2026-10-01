//! Parsing for the bounds validators carry as strings. The derive rejects a
//! bad bound at compile time; the generators and mockmake parse the same way.

use super::{
    BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator, StringValidator,
    Validator, parse_duration_to_nanos,
};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use std::cmp::Ordering;

/// A `YYYY-MM-DD` date bound, as the instant JavaScript gives it: midnight UTC.
pub fn date(bound: &str) -> Result<DateTime<Utc>, String> {
    NaiveDate::parse_from_str(bound, "%Y-%m-%d")
        .map(|date| date.and_time(NaiveTime::MIN).and_utc())
        .map_err(|error| format!("date bound {bound:?} is not a YYYY-MM-DD date: {error}"))
}

/// An integer bound.
pub fn big_int(bound: &str) -> Result<i128, String> {
    bound
        .parse()
        .map_err(|error| format!("bigint bound {bound:?} is not an integer: {error}"))
}

/// A SurrealDB-style duration bound (`1h30m`) in nanoseconds.
pub fn duration(bound: &str) -> Result<i128, String> {
    parse_duration_to_nanos(bound)
        .ok_or_else(|| format!("duration bound {bound:?} is not a duration such as 1h30m"))
}

/// An exact-length bound.
pub fn length(bound: &str) -> Result<usize, String> {
    bound
        .parse()
        .map_err(|error| format!("length bound {bound:?} is not a count: {error}"))
}

/// A decimal number: optional `-`, digits, and optionally `.` and digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal {
    negative: bool,
    whole: String,
    fraction: String,
}

impl Decimal {
    pub fn parse(text: &str) -> Option<Self> {
        let (negative, unsigned) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        let is_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
        if whole.is_empty() || !is_digits(whole) || !is_digits(fraction) {
            return None;
        }
        if unsigned.contains('.') && fraction.is_empty() {
            return None;
        }
        let whole = whole.trim_start_matches('0');
        let fraction = fraction.trim_end_matches('0');
        let is_zero = whole.is_empty() && fraction.is_empty();
        Some(Decimal {
            negative: negative && !is_zero,
            whole: whole.to_owned(),
            fraction: fraction.to_owned(),
        })
    }

    fn cmp_magnitude(&self, other: &Self) -> Ordering {
        self.whole
            .len()
            .cmp(&other.whole.len())
            .then_with(|| self.whole.cmp(&other.whole))
            .then_with(|| self.fraction.cmp(&other.fraction))
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => self.cmp_magnitude(other),
            (true, true) => other.cmp_magnitude(self),
        }
    }
}

/// A decimal bound, compared exactly.
pub fn decimal(bound: &str) -> Result<Decimal, String> {
    Decimal::parse(bound).ok_or_else(|| format!("decimal bound {bound:?} is not a decimal number"))
}

impl Validator {
    /// Rejects a bound that cannot be parsed, naming it.
    pub fn check_bounds(&self) -> Result<(), String> {
        match self {
            Validator::StringValidator(StringValidator::Length(bound)) => length(bound).map(drop),
            Validator::DateValidator(validator) => match validator {
                DateValidator::ValidDate => Ok(()),
                DateValidator::GreaterThanDate(bound)
                | DateValidator::GreaterThanOrEqualToDate(bound)
                | DateValidator::LessThanDate(bound)
                | DateValidator::LessThanOrEqualToDate(bound) => date(bound).map(drop),
                DateValidator::BetweenDate(start, end) => {
                    date(start)?;
                    date(end).map(drop)
                }
            },
            Validator::BigIntValidator(validator) => match validator {
                BigIntValidator::GreaterThanBigInt(bound)
                | BigIntValidator::GreaterThanOrEqualToBigInt(bound)
                | BigIntValidator::LessThanBigInt(bound)
                | BigIntValidator::LessThanOrEqualToBigInt(bound) => big_int(bound).map(drop),
                BigIntValidator::BetweenBigInt(start, end) => {
                    big_int(start)?;
                    big_int(end).map(drop)
                }
                BigIntValidator::PositiveBigInt
                | BigIntValidator::NonNegativeBigInt
                | BigIntValidator::NegativeBigInt
                | BigIntValidator::NonPositiveBigInt => Ok(()),
            },
            Validator::BigDecimalValidator(validator) => match validator {
                BigDecimalValidator::GreaterThanBigDecimal(bound)
                | BigDecimalValidator::GreaterThanOrEqualToBigDecimal(bound)
                | BigDecimalValidator::LessThanBigDecimal(bound)
                | BigDecimalValidator::LessThanOrEqualToBigDecimal(bound) => {
                    decimal(bound).map(drop)
                }
                BigDecimalValidator::BetweenBigDecimal(start, end) => {
                    decimal(start)?;
                    decimal(end).map(drop)
                }
                BigDecimalValidator::PositiveBigDecimal
                | BigDecimalValidator::NonNegativeBigDecimal
                | BigDecimalValidator::NegativeBigDecimal
                | BigDecimalValidator::NonPositiveBigDecimal => Ok(()),
            },
            Validator::DurationValidator(validator) => match validator {
                DurationValidator::GreaterThanDuration(bound)
                | DurationValidator::GreaterThanOrEqualToDuration(bound)
                | DurationValidator::LessThanDuration(bound)
                | DurationValidator::LessThanOrEqualToDuration(bound) => duration(bound).map(drop),
                DurationValidator::BetweenDuration(start, end) => {
                    duration(start)?;
                    duration(end).map(drop)
                }
            },
            Validator::StringValidator(_)
            | Validator::NumberValidator(_)
            | Validator::ArrayValidator(_) => Ok(()),
        }
    }
}

/// Checks a field's validators as a whole: every bound parses, and a parse
/// morph, which changes how the field is read, comes first and only once.
pub fn check_validators(validators: &[Validator]) -> Result<(), String> {
    for (index, validator) in validators.iter().enumerate() {
        validator.check_bounds()?;
        if index > 0
            && let Validator::StringValidator(string_validator) = validator
            && matches!(
                string_validator.rule(),
                super::string_rules::StringRule::Parse(_)
            )
        {
            return Err(format!(
                "{string_validator:?} parses the field's input, so it must be the first validator and appear once"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DateValidator, Decimal, DurationValidator, Validator};

    #[test]
    fn decimals_compare_exactly() {
        let parse = |text: &str| Decimal::parse(text).unwrap();
        assert!(parse("10.50") == parse("10.5"));
        assert!(parse("0.1") > parse("0.09999999999999999999"));
        assert!(parse("-2") < parse("-1.5"));
        assert!(parse("-0") == parse("0.000"));
        assert!(parse("100") > parse("99.99"));
        for rejected in ["", "-", "1.", ".5", "1e5", "NaN", "1,5"] {
            assert!(Decimal::parse(rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn bad_bounds_are_named() {
        let validator =
            Validator::DurationValidator(DurationValidator::GreaterThanDuration("soon".into()));
        assert!(validator.check_bounds().unwrap_err().contains("\"soon\""));
        assert!(
            Validator::DateValidator(DateValidator::BetweenDate(
                "2024-01-01".into(),
                "2024-02-30".into()
            ))
            .check_bounds()
            .is_err()
        );
    }
}
