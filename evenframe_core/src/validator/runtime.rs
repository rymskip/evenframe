//! Entry points for the validation the `Evenframe` derive generates. Each
//! validator family is bound by a trait, so a validator on a field type it
//! cannot apply to is a compile error rather than a silent pass. A newtype
//! meets each bound its inner type meets.

use super::bounds::{self, Decimal};
use super::keywords;
use super::morph::{StringMorph, round_to};
use super::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
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

// ----- Newtypes --------------------------------------------------------------

/// A struct serde writes as its one field's value. The derive implements it,
/// so a validator on a field holding the newtype checks the value inside.
pub trait Newtype {
    type Inner;
    fn inner(&self) -> &Self::Inner;
}

/// Rewriting and building a newtype's value, which skips the newtype's own
/// validators, so generated code checks the newtype again afterwards.
#[doc(hidden)]
pub trait NewtypeParts: Newtype + Sized {
    fn inner_mut(&mut self) -> &mut Self::Inner;
    fn from_inner(inner: Self::Inner) -> Self;
}

// ----- Strings ---------------------------------------------------------------

/// Text a [`StringValidator`] checks.
pub trait StringValue {
    fn text(&self) -> &str;
}

impl StringValue for str {
    fn text(&self) -> &str {
        self
    }
}

impl StringValue for String {
    fn text(&self) -> &str {
        self
    }
}

impl<N: Newtype> StringValue for N
where
    N::Inner: StringValue,
{
    fn text(&self) -> &str {
        self.inner().text()
    }
}

/// Text a string morph rewrites.
pub trait StringTarget {
    fn text_mut(&mut self) -> &mut String;
}

impl StringTarget for String {
    fn text_mut(&mut self) -> &mut String {
        self
    }
}

impl<N: NewtypeParts> StringTarget for N
where
    N::Inner: StringTarget,
{
    fn text_mut(&mut self) -> &mut String {
        self.inner_mut().text_mut()
    }
}

/// Applies a string check.
pub fn check_string<S: StringValue + ?Sized>(
    value: &S,
    validator: &StringValidator,
) -> Result<(), Rejection> {
    require(validator.accepts(value.text()), || validator.expectation())
}

/// Applies a string morph in place.
pub fn morph_string<S: StringTarget + ?Sized>(
    value: &mut S,
    morph: StringMorph,
) -> Result<(), Rejection> {
    let text = value.text_mut();
    *text = morph.apply(text);
    Ok(())
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

impl<N: Newtype> NumberValue for N
where
    N::Inner: NumberValue,
{
    fn as_f64(&self) -> f64 {
        self.inner().as_f64()
    }
    fn is_integer(&self) -> bool {
        self.inner().is_integer()
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
        NumberValidator::NonNan => (!number.is_nan(), "a number, not NaN".to_owned()),
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

/// The digits after the point in `number`'s shortest decimal form, which is
/// how Rust and SurrealDB both write a float, never with an exponent.
pub fn decimal_places(number: f64) -> usize {
    number
        .to_string()
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len())
}

/// Effect's `multipleOf`: the remainder after scaling both operands to
/// integers by the larger number of decimal places is zero.
pub fn is_multiple_of(value: f64, divisor: f64) -> bool {
    if divisor == 0.0 || !value.is_finite() || !divisor.is_finite() {
        return false;
    }
    let places = decimal_places(value).max(decimal_places(divisor));
    let Ok(exponent) = i32::try_from(places) else {
        return false;
    };
    let scale = 10_f64.powi(exponent);
    ((value * scale).round() % (divisor * scale).round()) == 0.0
}

// ----- Number morphs ---------------------------------------------------------

/// A number `round` rewrites, as JavaScript's `Math.round` rounds it.
pub trait RoundTarget {
    fn round_places(&mut self, places: u32);
}

impl RoundTarget for f64 {
    fn round_places(&mut self, places: u32) {
        *self = round_to(*self, places);
    }
}

impl<N: NewtypeParts> RoundTarget for N
where
    N::Inner: RoundTarget,
{
    fn round_places(&mut self, places: u32) {
        self.inner_mut().round_places(places);
    }
}

/// Rounds the value to `places` decimal places.
pub fn round_number<N: RoundTarget + ?Sized>(value: &mut N, places: u32) -> Result<(), Rejection> {
    value.round_places(places);
    Ok(())
}

/// A number, bigint or decimal `clamp` pulls into range, its bounds written
/// as decimals.
pub trait ClampTarget {
    fn clamp_between(&mut self, min: &str, max: &str) -> Result<(), Rejection>;
}

/// `min` and `max`, refused when they are out of order.
fn ordered_bounds<T: PartialOrd + fmt::Display>(min: T, max: T) -> Result<(T, T), Rejection> {
    if min > max {
        return Err(Rejection(format!(
            "clamp's minimum {min} is above its maximum {max}"
        )));
    }
    Ok((min, max))
}

/// An integer bound, refused when it has a fraction.
fn integer_bound(bound: &str) -> Result<i128, Rejection> {
    bound.parse().map_err(|_| {
        Rejection(format!(
            "clamp bound {bound} is not an integer, so it cannot bound an integer"
        ))
    })
}

macro_rules! integer_clamp_target {
    ($($integer:ty),*) => {$(
        impl ClampTarget for $integer {
            fn clamp_between(&mut self, min: &str, max: &str) -> Result<(), Rejection> {
                let (min, max) = ordered_bounds(integer_bound(min)?, integer_bound(max)?)?;
                let clamped = match i128::try_from(*self) {
                    Ok(current) if (min..=max).contains(&current) => return Ok(()),
                    Ok(current) => current.clamp(min, max),
                    // Only a `u128` or `usize` above `i128::MAX` fails, which
                    // is above every bound.
                    Err(_) => max,
                };
                *self = <$integer>::try_from(clamped).map_err(|_| {
                    Rejection(format!("clamp bound {clamped} does not fit a {}", stringify!($integer)))
                })?;
                Ok(())
            }
        }
    )*};
}

integer_clamp_target!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

impl ClampTarget for f64 {
    fn clamp_between(&mut self, min: &str, max: &str) -> Result<(), Rejection> {
        let parse = |bound: &str| {
            bound
                .parse::<f64>()
                .map_err(|error| Rejection(format!("clamp bound {bound} is not a number: {error}")))
        };
        let (min, max) = ordered_bounds(parse(min)?, parse(max)?)?;
        if self.is_nan() {
            return Err(Rejection::expected("a number"));
        }
        *self = self.max(min).min(max);
        Ok(())
    }
}

/// Decimal text, compared exactly and set to the bound it passes.
impl ClampTarget for String {
    fn clamp_between(&mut self, min: &str, max: &str) -> Result<(), Rejection> {
        let value = Decimal::parse(self).ok_or_else(|| Rejection::expected("a decimal number"))?;
        if value < bound(bounds::decimal(min))? {
            *self = min.to_owned();
        } else if value > bound(bounds::decimal(max))? {
            *self = max.to_owned();
        }
        Ok(())
    }
}

impl<N: NewtypeParts> ClampTarget for N
where
    N::Inner: ClampTarget,
{
    fn clamp_between(&mut self, min: &str, max: &str) -> Result<(), Rejection> {
        self.inner_mut().clamp_between(min, max)
    }
}

/// Pulls the value into `min..=max`.
pub fn clamp_number<N: ClampTarget + ?Sized>(
    value: &mut N,
    min: &str,
    max: &str,
) -> Result<(), Rejection> {
    value.clamp_between(min, max)
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

impl<N: Newtype> ItemCount for N
where
    N::Inner: ItemCount,
{
    fn item_count(&self) -> usize {
        self.inner().item_count()
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

/// A list `sort` orders.
pub trait SortTarget {
    fn sort_items(&mut self);
}

impl<T: Ord> SortTarget for Vec<T> {
    fn sort_items(&mut self) {
        self.sort();
    }
}

impl<N: NewtypeParts> SortTarget for N
where
    N::Inner: SortTarget,
{
    fn sort_items(&mut self) {
        self.inner_mut().sort_items();
    }
}

/// Orders the elements ascending.
pub fn sort_items<A: SortTarget + ?Sized>(value: &mut A) -> Result<(), Rejection> {
    value.sort_items();
    Ok(())
}

/// A list `unique` drops repeats from.
pub trait UniqueTarget {
    fn unique_items(&mut self);
}

impl<T: Ord> UniqueTarget for Vec<T> {
    fn unique_items(&mut self) {
        let first: Vec<bool> = {
            let mut seen = BTreeSet::new();
            self.iter().map(|item| seen.insert(item)).collect()
        };
        let mut first = first.into_iter();
        self.retain(|_| first.next().unwrap_or(false));
    }
}

impl<N: NewtypeParts> UniqueTarget for N
where
    N::Inner: UniqueTarget,
{
    fn unique_items(&mut self) {
        self.inner_mut().unique_items();
    }
}

/// Drops every element equal to an earlier one, keeping the first.
pub fn unique_items<A: UniqueTarget + ?Sized>(value: &mut A) -> Result<(), Rejection> {
    value.unique_items();
    Ok(())
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

impl<N: Newtype> InstantValue for N
where
    N::Inner: InstantValue,
{
    fn instant(&self) -> Option<DateTime<Utc>> {
        self.inner().instant()
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

impl<N: Newtype> BigIntValue for N
where
    N::Inner: BigIntValue,
{
    fn big_int(&self) -> Option<i128> {
        self.inner().big_int()
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

impl<N: Newtype> DecimalValue for N
where
    N::Inner: DecimalValue,
{
    fn decimal_text(&self) -> Cow<'_, str> {
        self.inner().decimal_text()
    }
}

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

impl<N: Newtype> DurationValue for N
where
    N::Inner: DurationValue,
{
    fn nanos(&self) -> Option<i128> {
        self.inner().nanos()
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
        BigDecimalValidator, DateValidator, DurationValidator, NumberValidator, StringMorph,
        TimeZone, Utc, check_date, check_decimal, check_duration, check_number, clamp_number,
        decimal_places, morph_string, round_number, sort_items, unique_items,
    };
    use ordered_float::OrderedFloat;

    #[test]
    fn morphs_rewrite_in_place() {
        let mut text = "  Mixed   Case  ".to_owned();
        morph_string(&mut text, StringMorph::Trim).unwrap();
        morph_string(&mut text, StringMorph::CollapseWhitespace).unwrap();
        morph_string(&mut text, StringMorph::Lower).unwrap();
        assert_eq!(text, "mixed case");

        let mut number = 2.345_f64;
        round_number(&mut number, 2).unwrap();
        assert_eq!(number, 2.35);
        clamp_number(&mut number, "0", "1.5").unwrap();
        assert_eq!(number, 1.5);

        let mut count = 300_u16;
        clamp_number(&mut count, "0", "255").unwrap();
        assert_eq!(count, 255);
        assert!(clamp_number(&mut count, "0.5", "255").is_err());
        let mut small = 3_u8;
        assert!(clamp_number(&mut small, "1000", "2000").is_err());
        let mut huge = u128::MAX;
        clamp_number(&mut huge, "0", &i128::MAX.to_string()).unwrap();
        assert_eq!(huge, i128::MAX.unsigned_abs());
        let mut missing = f64::NAN;
        assert!(clamp_number(&mut missing, "0", "1").is_err());
        assert!(clamp_number(&mut number, "2", "1").is_err());

        let mut decimal = "12.50".to_owned();
        clamp_number(&mut decimal, "0", "10.25").unwrap();
        assert_eq!(decimal, "10.25");

        let mut items = vec![3, 1, 3, 2, 1];
        unique_items(&mut items).unwrap();
        assert_eq!(items, vec![3, 1, 2]);
        sort_items(&mut items).unwrap();
        assert_eq!(items, vec![1, 2, 3]);
    }

    #[test]
    fn number_checks_accept_every_numeric_type() {
        assert!(check_number(&5_u8, &NumberValidator::Int).is_ok());
        assert!(check_number(&5.5_f64, &NumberValidator::Int).is_err());
        assert!(check_number(&10_i64, &NumberValidator::MultipleOf(OrderedFloat(2.5))).is_ok());
        assert!(check_number(&0.3_f64, &NumberValidator::MultipleOf(OrderedFloat(0.1))).is_ok());
        assert!(
            check_number(
                &0.30000000000000004_f64,
                &NumberValidator::MultipleOf(OrderedFloat(0.1))
            )
            .is_err()
        );
        assert_eq!(decimal_places(1.5e-7), 8);
        assert_eq!(decimal_places(1e21), 0);
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
