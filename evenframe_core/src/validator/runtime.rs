//! Entry points for the validation the `Evenframe` derive generates. Each
//! validator family is bound by a trait, so a validator on a field type it
//! cannot apply to is a compile error rather than a silent pass.

use super::bounds::{self, Decimal};
use super::keywords;
use super::string_rules::{StringParse, StringRule};
use super::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator,
};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt;

/// Why a value failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection(String);

impl Rejection {
    fn expected(expectation: impl fmt::Display) -> Self {
        Rejection(format!("must be {expectation}"))
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Rejection {}

fn require(passes: bool, expectation: impl FnOnce() -> String) -> Result<(), Rejection> {
    if passes {
        Ok(())
    } else {
        Err(Rejection::expected(expectation()))
    }
}

fn bound<T>(parsed: Result<T, String>) -> Result<T, Rejection> {
    parsed.map_err(Rejection)
}

// ----- Strings ---------------------------------------------------------------

/// Applies a string check.
pub fn check_string(value: &str, validator: &StringValidator) -> Result<(), Rejection> {
    require(validator.accepts(value), || validator.expectation())
}

/// Applies a string transform in place.
pub fn transform_string(value: &mut String, validator: &StringValidator) -> Result<(), Rejection> {
    match validator.rule() {
        StringRule::Transform(transform) => {
            *value = transform.apply(value);
            Ok(())
        }
        StringRule::Check | StringRule::Parse(_) | StringRule::Carrier => {
            Err(Rejection(format!("{validator:?} is not a transform")))
        }
    }
}

// ----- Parse morphs ----------------------------------------------------------

/// A type `string.integer.parse` can produce.
pub trait FromSafeInteger: Sized {
    fn from_safe_integer(value: i64) -> Option<Self>;
}

macro_rules! from_safe_integer_via_try_from {
    ($($target:ty),*) => {$(
        impl FromSafeInteger for $target {
            fn from_safe_integer(value: i64) -> Option<Self> {
                <$target>::try_from(value).ok()
            }
        }
    )*};
}

from_safe_integer_via_try_from!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl FromSafeInteger for f64 {
    fn from_safe_integer(value: i64) -> Option<Self> {
        // Every safe integer is exactly representable as an f64.
        Some(value as f64)
    }
}

/// A type `string.numeric.parse` can produce.
pub trait FromNumeric: Sized {
    fn from_numeric(text: &str) -> Option<Self>;
}

impl FromNumeric for f64 {
    fn from_numeric(text: &str) -> Option<Self> {
        keywords::parse_numeric(text)
    }
}

impl FromNumeric for f32 {
    fn from_numeric(text: &str) -> Option<Self> {
        keywords::parse_numeric(text).and_then(|_| text.parse().ok())
    }
}

/// A type the date parse morphs can produce.
pub trait FromInstant: Sized {
    fn from_instant(instant: DateTime<Utc>) -> Self;
}

impl FromInstant for DateTime<Utc> {
    fn from_instant(instant: DateTime<Utc>) -> Self {
        instant
    }
}

impl FromInstant for DateTime<FixedOffset> {
    fn from_instant(instant: DateTime<Utc>) -> Self {
        instant.fixed_offset()
    }
}

impl FromInstant for NaiveDateTime {
    fn from_instant(instant: DateTime<Utc>) -> Self {
        instant.naive_utc()
    }
}

/// Reads `raw` with a parse morph.
pub fn parse_integer<T: FromSafeInteger>(raw: &str) -> Result<T, Rejection> {
    keywords::parse_safe_integer(raw)
        .and_then(T::from_safe_integer)
        .ok_or_else(|| Rejection::expected(StringValidator::IntegerParse.description()))
}

pub fn parse_numeric<T: FromNumeric>(raw: &str) -> Result<T, Rejection> {
    T::from_numeric(raw)
        .ok_or_else(|| Rejection::expected(StringValidator::NumericParse.description()))
}

pub fn parse_date<T: FromInstant>(raw: &str) -> Result<T, Rejection> {
    keywords::parse_date(raw)
        .map(T::from_instant)
        .ok_or_else(|| Rejection::expected(StringValidator::DateParse.description()))
}

pub fn parse_date_iso<T: FromInstant>(raw: &str) -> Result<T, Rejection> {
    require(keywords::is_iso_8601(raw), || {
        StringValidator::DateIsoParse.description().to_owned()
    })?;
    parse_date(raw)
}

pub fn parse_date_epoch<T: FromInstant>(raw: &str) -> Result<T, Rejection> {
    keywords::parse_epoch_millis(raw)
        .and_then(|millis| Utc.timestamp_millis_opt(millis).single())
        .map(T::from_instant)
        .ok_or_else(|| Rejection::expected(StringValidator::DateEpochParse.description()))
}

pub fn parse_json<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, Rejection> {
    if raw.is_empty() {
        return Err(Rejection::expected("a JSON string, not an empty one"));
    }
    serde_json::from_str(raw).map_err(|error| Rejection(format!("must be a JSON string ({error})")))
}

pub fn parse_url(raw: &str) -> Result<url::Url, Rejection> {
    url::Url::parse(raw).map_err(|error| Rejection(format!("must be a URL string ({error})")))
}

impl StringParse {
    /// The runtime function the derive calls for this parse.
    pub fn runtime_function(self) -> &'static str {
        match self {
            StringParse::Integer => "parse_integer",
            StringParse::Numeric => "parse_numeric",
            StringParse::Date => "parse_date",
            StringParse::DateIso => "parse_date_iso",
            StringParse::DateEpoch => "parse_date_epoch",
            StringParse::Json => "parse_json",
            StringParse::Url => "parse_url",
        }
    }
}

// ----- Numbers ---------------------------------------------------------------

/// A number a [`NumberValidator`] applies to, compared as JavaScript does:
/// as an f64.
pub trait NumberValue {
    fn as_f64(&self) -> f64;
    fn is_integer(&self) -> bool;
}

macro_rules! integer_number_value {
    ($($integer:ty),*) => {$(
        impl NumberValue for $integer {
            fn as_f64(&self) -> f64 {
                *self as f64
            }
            fn is_integer(&self) -> bool {
                true
            }
        }
    )*};
}

integer_number_value!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl NumberValue for f64 {
    fn as_f64(&self) -> f64 {
        *self
    }
    fn is_integer(&self) -> bool {
        self.is_finite() && self.fract() == 0.0
    }
}

impl NumberValue for f32 {
    fn as_f64(&self) -> f64 {
        f64::from(*self)
    }
    fn is_integer(&self) -> bool {
        self.is_finite() && self.fract() == 0.0
    }
}

pub fn check_number<N: NumberValue + ?Sized>(
    value: &N,
    validator: &NumberValidator,
) -> Result<(), Rejection> {
    let number = value.as_f64();
    let (passes, expectation) = match validator {
        NumberValidator::GreaterThan(bound) => (number > bound.0, format!("more than {}", bound.0)),
        NumberValidator::GreaterThanOrEqualTo(bound) => {
            (number >= bound.0, format!("at least {}", bound.0))
        }
        NumberValidator::LessThan(bound) => (number < bound.0, format!("less than {}", bound.0)),
        NumberValidator::LessThanOrEqualTo(bound) => {
            (number <= bound.0, format!("at most {}", bound.0))
        }
        NumberValidator::Between(start, end) => (
            number >= start.0 && number <= end.0,
            format!("between {} and {}", start.0, end.0),
        ),
        NumberValidator::Int => (value.is_integer(), "an integer".to_owned()),
        NumberValidator::NonNaN => (!number.is_nan(), "a number, not NaN".to_owned()),
        NumberValidator::Finite => (number.is_finite(), "a finite number".to_owned()),
        NumberValidator::Positive => (number > 0.0, "positive".to_owned()),
        NumberValidator::NonNegative => (number >= 0.0, "non-negative".to_owned()),
        NumberValidator::Negative => (number < 0.0, "negative".to_owned()),
        NumberValidator::NonPositive => (number <= 0.0, "non-positive".to_owned()),
        NumberValidator::MultipleOf(divisor) => (
            is_multiple_of(number, divisor.0),
            format!("a multiple of {}", divisor.0),
        ),
        NumberValidator::Uint8 => (
            value.is_integer() && (0.0..=255.0).contains(&number),
            "an integer from 0 to 255".to_owned(),
        ),
    };
    require(passes, || expectation)
}

/// Effect's `multipleOf`: the remainder after scaling both operands to
/// integers by the larger number of decimal places is zero.
pub fn is_multiple_of(value: f64, divisor: f64) -> bool {
    if divisor == 0.0 || !value.is_finite() || !divisor.is_finite() {
        return false;
    }
    let decimals = |number: f64| {
        let text = number.to_string();
        text.split_once('.')
            .map_or(0, |(_, fraction)| fraction.len())
    };
    let places = decimals(value).max(decimals(divisor));
    let Ok(exponent) = i32::try_from(places) else {
        return false;
    };
    let scale = 10_f64.powi(exponent);
    ((value * scale).round() % (divisor * scale).round()) == 0.0
}

// ----- Collections -----------------------------------------------------------

/// A collection an [`ArrayValidator`] counts.
pub trait ItemCount {
    fn item_count(&self) -> usize;
}

impl<T> ItemCount for [T] {
    fn item_count(&self) -> usize {
        self.len()
    }
}

impl<T, const LENGTH: usize> ItemCount for [T; LENGTH] {
    fn item_count(&self) -> usize {
        LENGTH
    }
}

macro_rules! len_item_count {
    ($($collection:ident<$($parameter:ident),*>),*) => {$(
        impl<$($parameter),*> ItemCount for $collection<$($parameter),*> {
            fn item_count(&self) -> usize {
                self.len()
            }
        }
    )*};
}

len_item_count!(Vec<T>, VecDeque<T>, BTreeSet<T>, BTreeMap<K, V>);

impl<T, S> ItemCount for HashSet<T, S> {
    fn item_count(&self) -> usize {
        self.len()
    }
}

impl<K, V, S> ItemCount for HashMap<K, V, S> {
    fn item_count(&self) -> usize {
        self.len()
    }
}

pub fn check_items<C: ItemCount + ?Sized>(
    value: &C,
    validator: &ArrayValidator,
) -> Result<(), Rejection> {
    let count = value.item_count();
    match validator {
        ArrayValidator::MinItems(minimum) => {
            require(count >= *minimum, || format!("at least {minimum} items"))
        }
        ArrayValidator::MaxItems(maximum) => {
            require(count <= *maximum, || format!("at most {maximum} items"))
        }
        ArrayValidator::ItemsCount(exact) => {
            require(count == *exact, || format!("exactly {exact} items"))
        }
    }
}

// ----- Dates -----------------------------------------------------------------

/// A value a [`DateValidator`] compares, as the instant JavaScript's `Date`
/// would hold.
pub trait InstantValue {
    /// `None` when the value is not a valid date.
    fn instant(&self) -> Option<DateTime<Utc>>;
}

impl<Tz: TimeZone> InstantValue for DateTime<Tz> {
    fn instant(&self) -> Option<DateTime<Utc>> {
        Some(self.with_timezone(&Utc))
    }
}

impl InstantValue for NaiveDateTime {
    fn instant(&self) -> Option<DateTime<Utc>> {
        Some(self.and_utc())
    }
}

impl InstantValue for NaiveDate {
    fn instant(&self) -> Option<DateTime<Utc>> {
        Some(self.and_time(NaiveTime::MIN).and_utc())
    }
}

impl InstantValue for str {
    fn instant(&self) -> Option<DateTime<Utc>> {
        keywords::parse_date(self)
    }
}

impl InstantValue for String {
    fn instant(&self) -> Option<DateTime<Utc>> {
        keywords::parse_date(self)
    }
}

pub fn check_date<D: InstantValue + ?Sized>(
    value: &D,
    validator: &DateValidator,
) -> Result<(), Rejection> {
    let instant = value
        .instant()
        .ok_or_else(|| Rejection::expected("a valid date"))?;
    match validator {
        DateValidator::ValidDate => Ok(()),
        DateValidator::GreaterThanDate(start) => {
            require(instant > bound(bounds::date(start))?, || {
                format!("after {start}")
            })
        }
        DateValidator::GreaterThanOrEqualToDate(start) => {
            require(instant >= bound(bounds::date(start))?, || {
                format!("on or after {start}")
            })
        }
        DateValidator::LessThanDate(end) => require(instant < bound(bounds::date(end))?, || {
            format!("before {end}")
        }),
        DateValidator::LessThanOrEqualToDate(end) => {
            require(instant <= bound(bounds::date(end))?, || {
                format!("on or before {end}")
            })
        }
        DateValidator::BetweenDate(start, end) => {
            let range = bound(bounds::date(start))?..=bound(bounds::date(end))?;
            require(range.contains(&instant), || {
                format!("between {start} and {end}")
            })
        }
    }
}

// ----- BigInts ---------------------------------------------------------------

/// An integer a [`BigIntValidator`] compares.
pub trait BigIntValue {
    /// `None` when the value is not an integer or exceeds `i128`.
    fn big_int(&self) -> Option<i128>;
}

macro_rules! integer_big_int_value {
    ($($integer:ty),*) => {$(
        impl BigIntValue for $integer {
            fn big_int(&self) -> Option<i128> {
                i128::try_from(*self).ok()
            }
        }
    )*};
}

integer_big_int_value!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl BigIntValue for str {
    fn big_int(&self) -> Option<i128> {
        self.parse().ok()
    }
}

impl BigIntValue for String {
    fn big_int(&self) -> Option<i128> {
        self.parse().ok()
    }
}

pub fn check_big_int<B: BigIntValue + ?Sized>(
    value: &B,
    validator: &BigIntValidator,
) -> Result<(), Rejection> {
    let number = value
        .big_int()
        .ok_or_else(|| Rejection::expected("an integer"))?;
    match validator {
        BigIntValidator::GreaterThanBigInt(start) => {
            require(number > bound(bounds::big_int(start))?, || {
                format!("more than {start}")
            })
        }
        BigIntValidator::GreaterThanOrEqualToBigInt(start) => {
            require(number >= bound(bounds::big_int(start))?, || {
                format!("at least {start}")
            })
        }
        BigIntValidator::LessThanBigInt(end) => {
            require(number < bound(bounds::big_int(end))?, || {
                format!("less than {end}")
            })
        }
        BigIntValidator::LessThanOrEqualToBigInt(end) => {
            require(number <= bound(bounds::big_int(end))?, || {
                format!("at most {end}")
            })
        }
        BigIntValidator::BetweenBigInt(start, end) => {
            let range = bound(bounds::big_int(start))?..=bound(bounds::big_int(end))?;
            require(range.contains(&number), || {
                format!("between {start} and {end}")
            })
        }
        BigIntValidator::PositiveBigInt => require(number > 0, || "positive".to_owned()),
        BigIntValidator::NonNegativeBigInt => require(number >= 0, || "non-negative".to_owned()),
        BigIntValidator::NegativeBigInt => require(number < 0, || "negative".to_owned()),
        BigIntValidator::NonPositiveBigInt => require(number <= 0, || "non-positive".to_owned()),
    }
}

// ----- Decimals --------------------------------------------------------------

/// A decimal a [`BigDecimalValidator`] compares exactly, through its text.
pub trait DecimalValue {
    fn decimal_text(&self) -> Cow<'_, str>;
}

impl DecimalValue for str {
    fn decimal_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

impl DecimalValue for String {
    fn decimal_text(&self) -> Cow<'_, str> {
        Cow::Borrowed(self)
    }
}

macro_rules! integer_decimal_value {
    ($($integer:ty),*) => {$(
        impl DecimalValue for $integer {
            fn decimal_text(&self) -> Cow<'_, str> {
                Cow::Owned(self.to_string())
            }
        }
    )*};
}

integer_decimal_value!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

pub fn check_decimal<D: DecimalValue + ?Sized>(
    value: &D,
    validator: &BigDecimalValidator,
) -> Result<(), Rejection> {
    let number = Decimal::parse(&value.decimal_text())
        .ok_or_else(|| Rejection::expected("a decimal number"))?;
    let zero = Decimal::parse("0").ok_or_else(|| Rejection("zero is a decimal".to_owned()))?;
    match validator {
        BigDecimalValidator::GreaterThanBigDecimal(start) => {
            require(number > bound(bounds::decimal(start))?, || {
                format!("more than {start}")
            })
        }
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(start) => {
            require(number >= bound(bounds::decimal(start))?, || {
                format!("at least {start}")
            })
        }
        BigDecimalValidator::LessThanBigDecimal(end) => {
            require(number < bound(bounds::decimal(end))?, || {
                format!("less than {end}")
            })
        }
        BigDecimalValidator::LessThanOrEqualToBigDecimal(end) => {
            require(number <= bound(bounds::decimal(end))?, || {
                format!("at most {end}")
            })
        }
        BigDecimalValidator::BetweenBigDecimal(start, end) => {
            let range = bound(bounds::decimal(start))?..=bound(bounds::decimal(end))?;
            require(range.contains(&number), || {
                format!("between {start} and {end}")
            })
        }
        BigDecimalValidator::PositiveBigDecimal => require(number > zero, || "positive".to_owned()),
        BigDecimalValidator::NonNegativeBigDecimal => {
            require(number >= zero, || "non-negative".to_owned())
        }
        BigDecimalValidator::NegativeBigDecimal => require(number < zero, || "negative".to_owned()),
        BigDecimalValidator::NonPositiveBigDecimal => {
            require(number <= zero, || "non-positive".to_owned())
        }
    }
}

// ----- Durations -------------------------------------------------------------

/// A duration a [`DurationValidator`] compares, in nanoseconds.
pub trait DurationValue {
    fn nanos(&self) -> Option<i128>;
}

impl DurationValue for chrono::TimeDelta {
    fn nanos(&self) -> Option<i128> {
        self.num_nanoseconds()
            .map(i128::from)
            .or_else(|| i128::from(self.num_microseconds()?).checked_mul(1_000))
    }
}

impl DurationValue for std::time::Duration {
    fn nanos(&self) -> Option<i128> {
        i128::try_from(self.as_nanos()).ok()
    }
}

#[cfg(feature = "surrealdb-types")]
impl DurationValue for surrealdb_types::Duration {
    fn nanos(&self) -> Option<i128> {
        i128::try_from(surrealdb_types::Duration::nanos(self)).ok()
    }
}

/// A duration already counted in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nanos(pub i128);

impl DurationValue for Nanos {
    fn nanos(&self) -> Option<i128> {
        Some(self.0)
    }
}

impl DurationValue for str {
    fn nanos(&self) -> Option<i128> {
        super::parse_duration_to_nanos(self)
    }
}

impl DurationValue for String {
    fn nanos(&self) -> Option<i128> {
        super::parse_duration_to_nanos(self)
    }
}

pub fn check_duration<D: DurationValue + ?Sized>(
    value: &D,
    validator: &DurationValidator,
) -> Result<(), Rejection> {
    let nanos = value
        .nanos()
        .ok_or_else(|| Rejection::expected("a duration"))?;
    match validator {
        DurationValidator::GreaterThanDuration(start) => {
            require(nanos > bound(bounds::duration(start))?, || {
                format!("longer than {start}")
            })
        }
        DurationValidator::GreaterThanOrEqualToDuration(start) => {
            require(nanos >= bound(bounds::duration(start))?, || {
                format!("at least {start} long")
            })
        }
        DurationValidator::LessThanDuration(end) => {
            require(nanos < bound(bounds::duration(end))?, || {
                format!("shorter than {end}")
            })
        }
        DurationValidator::LessThanOrEqualToDuration(end) => {
            require(nanos <= bound(bounds::duration(end))?, || {
                format!("at most {end} long")
            })
        }
        DurationValidator::BetweenDuration(start, end) => {
            let range = bound(bounds::duration(start))?..=bound(bounds::duration(end))?;
            require(range.contains(&nanos), || {
                format!("between {start} and {end} long")
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BigDecimalValidator, DateTime, DateValidator, DurationValidator, NumberValidator,
        StringValidator, TimeZone, Utc, check_date, check_decimal, check_duration, check_number,
        parse_date_epoch, parse_date_iso, parse_integer, parse_json, parse_numeric,
        transform_string,
    };
    use ordered_float::OrderedFloat;

    #[test]
    fn transforms_rewrite_in_place() {
        let mut value = "  Mixed Case  ".to_owned();
        transform_string(&mut value, &StringValidator::Trim).unwrap();
        transform_string(&mut value, &StringValidator::Lower).unwrap();
        assert_eq!(value, "mixed case");
        assert!(transform_string(&mut value, &StringValidator::Email).is_err());
    }

    #[test]
    fn parse_morphs_produce_the_field_type() {
        assert_eq!(parse_integer::<i64>("42"), Ok(42));
        assert!(parse_integer::<u8>("300").is_err());
        assert!(parse_integer::<i64>("4.2").is_err());
        assert_eq!(parse_numeric::<f64>(".5"), Ok(0.5));
        let instant: DateTime<Utc> = parse_date_epoch("0").unwrap();
        assert_eq!(instant.timestamp(), 0);
        assert!(parse_date_iso::<DateTime<Utc>>("yesterday").is_err());
        let list: Vec<u8> = parse_json("[1,2]").unwrap();
        assert_eq!(list, vec![1, 2]);
        assert!(parse_json::<Vec<u8>>("").is_err());
    }

    #[test]
    fn number_checks_accept_every_numeric_type() {
        assert!(check_number(&5_u8, &NumberValidator::Int).is_ok());
        assert!(check_number(&5.5_f64, &NumberValidator::Int).is_err());
        assert!(check_number(&10_i64, &NumberValidator::MultipleOf(OrderedFloat(2.5))).is_ok());
        assert!(check_number(&0.3_f64, &NumberValidator::MultipleOf(OrderedFloat(0.1))).is_ok());
        assert!(check_number(&300_u16, &NumberValidator::Uint8).is_err());
    }

    #[test]
    fn dates_compare_as_instants() {
        let noon = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();
        let after = DateValidator::GreaterThanDate("2024-01-01".into());
        assert!(check_date(&noon, &after).is_ok());
        assert!(check_date("2023-12-31", &after).is_err());
    }

    #[test]
    fn decimals_and_durations_compare_exactly() {
        let minimum = BigDecimalValidator::GreaterThanOrEqualToBigDecimal("10.50".into());
        assert!(check_decimal("10.5", &minimum).is_ok());
        assert!(check_decimal("10.49999999999999999999", &minimum).is_err());
        let window = DurationValidator::BetweenDuration("1h".into(), "2h".into());
        assert!(check_duration(&chrono::TimeDelta::minutes(90), &window).is_ok());
        assert!(check_duration("3h", &window).is_err());
        let ninety_minutes = std::time::Duration::from_secs(90 * 60);
        assert!(check_duration(&ninety_minutes, &window).is_ok());
        assert!(check_duration(&std::time::Duration::from_secs(60), &window).is_err());
    }

    #[cfg(feature = "surrealdb-types")]
    #[test]
    fn sdk_durations_are_checked() {
        let window = DurationValidator::BetweenDuration("1h".into(), "2h".into());
        assert!(check_duration(&surrealdb_types::Duration::new(90 * 60, 0), &window).is_ok());
        assert!(check_duration(&surrealdb_types::Duration::new(3 * 3600, 1), &window).is_err());
    }
}
