//! What a `#[format(...)]` field's mock values look like: a [`Format`]'s
//! pattern, or a duration drawn from a range in fixed steps.

use super::format::Format;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MockFormat {
    Format(Format),
    /// A duration field's value, in `step`s from `min` to `max`.
    DurationNs(DurationRange),
}

impl MockFormat {
    /// The format whose pattern the values match, when they follow one.
    pub fn as_format(&self) -> Option<&Format> {
        match self {
            MockFormat::Format(format) => Some(format),
            MockFormat::DurationNs(_) => None,
        }
    }
}

impl ToTokens for MockFormat {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(match self {
            MockFormat::Format(format) => {
                quote! { ::evenframe::schemasync::mockmake::mock_format::MockFormat::Format(#format) }
            }
            MockFormat::DurationNs(range) => {
                let (min, max, step) = (range.min.nanos, range.max.nanos, range.step.nanos);
                quote! {
                    ::evenframe::schemasync::mockmake::mock_format::MockFormat::DurationNs(
                        ::evenframe::schemasync::mockmake::mock_format::DurationRange::checked(
                            #min, #max, #step
                        )
                    )
                }
            }
        });
    }
}

/// A fixed-length ISO 8601 duration: weeks, or days and a time of day. Years
/// and months are refused, since their length depends on the calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IsoDuration {
    nanos: u64,
}

/// The fixed-length forms. `iso8601` alone accepts trailing text and longer
/// fractions, which it truncates, so the shape is checked first.
static FIXED_LENGTH: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"^P(?:[0-9]+W|(?:[0-9]+D)?(?:T(?:[0-9]+H)?(?:[0-9]+M)?(?:[0-9]+(?:[.,][0-9]{1,3})?S)?)?)$",
    )
    .expect("the fixed-length duration pattern is a valid regex")
});

const NANOS_PER_MILLI: u64 = 1_000_000;
const NANOS_PER_SECOND: u64 = 1_000 * NANOS_PER_MILLI;
const NANOS_PER_MINUTE: u64 = 60 * NANOS_PER_SECOND;
const NANOS_PER_HOUR: u64 = 60 * NANOS_PER_MINUTE;
const NANOS_PER_DAY: u64 = 24 * NANOS_PER_HOUR;

impl IsoDuration {
    pub fn parse(written: &str) -> Result<Self, String> {
        let date_part = written
            .strip_prefix('P')
            .map(|rest| rest.split_once('T').map_or(rest, |(date, _)| date));
        if date_part.is_some_and(|date| date.contains('Y') || date.contains('M')) {
            return Err(format!(
                "`{written}` counts years or months, whose length depends on the calendar: \
                 write it in weeks, days, hours, minutes and seconds"
            ));
        }
        let empty = matches!(written, "P" | "PT") || written.ends_with('T');
        if empty || !FIXED_LENGTH.is_match(written) {
            return Err(format!(
                "`{written}` is not an ISO 8601 duration such as \"PT1H30M\", \"P1DT12H\" or \
                 \"P2W\", with at most three digits after a decimal point"
            ));
        }
        let parsed: iso8601::Duration = written
            .parse()
            .map_err(|error| format!("`{written}` is not an ISO 8601 duration: {error}"))?;
        let nanos = u64::try_from(std::time::Duration::from(parsed).as_nanos())
            .map_err(|_| format!("`{written}` is longer than a duration can hold"))?;
        Ok(Self { nanos })
    }

    pub fn nanos(self) -> u64 {
        self.nanos
    }
}

/// The shortest form: days, then hours, minutes and seconds, where a day is
/// 24 hours.
impl fmt::Display for IsoDuration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let days = self.nanos / NANOS_PER_DAY;
        let hours = self.nanos % NANOS_PER_DAY / NANOS_PER_HOUR;
        let minutes = self.nanos % NANOS_PER_HOUR / NANOS_PER_MINUTE;
        let seconds = self.nanos % NANOS_PER_MINUTE / NANOS_PER_SECOND;
        let millis = self.nanos % NANOS_PER_SECOND / NANOS_PER_MILLI;
        formatter.write_str("P")?;
        if days > 0 {
            write!(formatter, "{days}D")?;
        }
        if self.nanos.is_multiple_of(NANOS_PER_DAY) && days > 0 {
            return Ok(());
        }
        formatter.write_str("T")?;
        if hours > 0 {
            write!(formatter, "{hours}H")?;
        }
        if minutes > 0 {
            write!(formatter, "{minutes}M")?;
        }
        if millis > 0 {
            let fraction = format!("{millis:03}");
            write!(formatter, "{seconds}.{}S", fraction.trim_end_matches('0'))?;
        } else if seconds > 0 || self.nanos == 0 {
            write!(formatter, "{seconds}S")?;
        }
        Ok(())
    }
}

impl Serialize for IsoDuration {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for IsoDuration {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let written = String::deserialize(deserializer)?;
        Self::parse(&written).map_err(serde::de::Error::custom)
    }
}

/// Durations from `min` to `max` in `step`s, both ends included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "DurationRangeWire")]
pub struct DurationRange {
    min: IsoDuration,
    max: IsoDuration,
    step: IsoDuration,
}

#[derive(Deserialize)]
struct DurationRangeWire {
    min: IsoDuration,
    max: IsoDuration,
    step: IsoDuration,
}

impl TryFrom<DurationRangeWire> for DurationRange {
    type Error = String;

    fn try_from(wire: DurationRangeWire) -> Result<Self, Self::Error> {
        Self::new(wire.min, wire.max, wire.step)
    }
}

impl DurationRange {
    pub fn new(min: IsoDuration, max: IsoDuration, step: IsoDuration) -> Result<Self, String> {
        if step.nanos == 0 {
            return Err("the step of a duration range must be longer than zero".to_owned());
        }
        if min > max {
            return Err(format!(
                "the duration range starts at {min}, after it ends at {max}"
            ));
        }
        if !(max.nanos - min.nanos).is_multiple_of(step.nanos) {
            return Err(format!(
                "steps of {step} from {min} do not land on {max}: the step must divide the range"
            ));
        }
        Ok(Self { min, max, step })
    }

    /// A range already checked where its attribute was parsed. Only derive
    /// output calls this.
    #[doc(hidden)]
    pub const fn checked(min: u64, max: u64, step: u64) -> Self {
        Self {
            min: IsoDuration { nanos: min },
            max: IsoDuration { nanos: max },
            step: IsoDuration { nanos: step },
        }
    }

    pub fn min(&self) -> IsoDuration {
        self.min
    }

    pub fn max(&self) -> IsoDuration {
        self.max
    }

    pub fn step(&self) -> IsoDuration {
        self.step
    }

    /// The range written as an attribute, `duration_ns(min = "PT1H", max =
    /// "PT5H", step = "PT15M")`, or `None` for an expression of another form.
    pub fn from_attribute(expr: &syn::Expr) -> Result<Option<Self>, syn::Error> {
        let syn::Expr::Call(call) = expr else {
            return Ok(None);
        };
        let syn::Expr::Path(function) = &*call.func else {
            return Ok(None);
        };
        if !(function.path.is_ident("duration_ns") || function.path.is_ident("DurationNs")) {
            return Ok(None);
        }
        let (mut min, mut max, mut step) = (None, None, None);
        for argument in &call.args {
            let syn::Expr::Assign(assign) = argument else {
                return Err(syn::Error::new_spanned(
                    argument,
                    "write each bound as `min = \"PT1H\"`, `max = ...` or `step = ...`",
                ));
            };
            let slot = match &*assign.left {
                syn::Expr::Path(name) if name.path.is_ident("min") => &mut min,
                syn::Expr::Path(name) if name.path.is_ident("max") => &mut max,
                syn::Expr::Path(name) if name.path.is_ident("step") => &mut step,
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "a duration range takes `min`, `max` and `step`",
                    ));
                }
            };
            if slot.is_some() {
                return Err(syn::Error::new_spanned(
                    &assign.left,
                    "this bound is given twice",
                ));
            }
            let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(written),
                ..
            }) = &*assign.right
            else {
                return Err(syn::Error::new_spanned(
                    &assign.right,
                    "a bound is an ISO 8601 duration in quotes, such as \"PT15M\"",
                ));
            };
            *slot = Some(
                IsoDuration::parse(&written.value())
                    .map_err(|message| syn::Error::new_spanned(written, message))?,
            );
        }
        let (Some(min), Some(max), Some(step)) = (min, max, step) else {
            return Err(syn::Error::new_spanned(
                call,
                "a duration range takes all of `min`, `max` and `step`, such as \
                 `duration_ns(min = \"PT1H\", max = \"PT5H\", step = \"PT15M\")`",
            ));
        };
        Self::new(min, max, step)
            .map(Some)
            .map_err(|message| syn::Error::new_spanned(call, message))
    }
}

#[cfg(feature = "mockmake")]
impl DurationRange {
    /// A duration in the range, in nanoseconds.
    pub fn generate(&self, rng: &mut impl rand::RngExt) -> u64 {
        let steps = (self.max.nanos - self.min.nanos) / self.step.nanos;
        self.min.nanos + rng.random_range(0..=steps) * self.step.nanos
    }
}

#[cfg(test)]
mod tests {
    use super::{DurationRange, IsoDuration, MockFormat};

    fn duration(written: &str) -> IsoDuration {
        IsoDuration::parse(written).expect("a fixed-length duration")
    }

    fn attribute(written: &str) -> Result<Option<DurationRange>, String> {
        DurationRange::from_attribute(&syn::parse_str(written).expect("an expression"))
            .map_err(|error| error.to_string())
    }

    #[test]
    fn fixed_length_durations_read_and_print_in_their_shortest_form() {
        assert_eq!(duration("PT1H").nanos(), 3_600_000_000_000);
        assert_eq!(duration("PT15M").nanos(), 900_000_000_000);
        assert_eq!(duration("P2W").nanos(), 14 * 86_400_000_000_000);
        for (written, shortest) in [
            ("PT1H", "PT1H"),
            ("PT90M", "PT1H30M"),
            ("P1DT2H", "P1DT2H"),
            ("PT24H", "P1D"),
            ("P1W", "P7D"),
            ("PT1.5S", "PT1.5S"),
            ("PT0.250S", "PT0.25S"),
            ("PT0S", "PT0S"),
        ] {
            assert_eq!(duration(written).to_string(), shortest, "{written}");
            assert_eq!(duration(shortest), duration(written), "{shortest}");
        }
    }

    #[test]
    fn calendar_lengths_and_malformed_durations_are_refused() {
        for written in ["P1Y", "P2M", "P1MT1H"] {
            let message = IsoDuration::parse(written).expect_err(written);
            assert!(message.contains("years or months"), "{written}: {message}");
        }
        for written in ["1h", "P", "PT", "P1DT", "PT1Hx", "PT1.2345S", "P1W2D", ""] {
            let message = IsoDuration::parse(written).expect_err(written);
            assert!(
                message.contains("not an ISO 8601 duration"),
                "{written}: {message}"
            );
        }
    }

    #[test]
    fn a_range_holds_whole_steps_from_its_start_to_its_end() {
        let range = DurationRange::new(duration("PT1H"), duration("PT5H"), duration("PT15M"))
            .expect("a range");
        assert_eq!(range.max().nanos(), 18_000_000_000_000);
        assert!(
            DurationRange::new(duration("PT1H"), duration("PT5H"), duration("PT0S"))
                .expect_err("a zero step")
                .contains("longer than zero")
        );
        assert!(
            DurationRange::new(duration("PT5H"), duration("PT1H"), duration("PT15M"))
                .expect_err("a reversed range")
                .contains("after it ends")
        );
        assert!(
            DurationRange::new(duration("PT1H"), duration("PT2H"), duration("PT25M"))
                .expect_err("a step that misses the end")
                .contains("must divide the range")
        );
    }

    #[cfg(feature = "mockmake")]
    #[test]
    fn generated_durations_land_on_a_step_inside_the_range() {
        let range = DurationRange::new(duration("PT1H"), duration("PT5H"), duration("PT15M"))
            .expect("a range");
        let mut rng = rand::rng();
        for _ in 0..200 {
            let nanos = range.generate(&mut rng);
            assert!((3_600_000_000_000..=18_000_000_000_000).contains(&nanos));
            assert!((nanos - 3_600_000_000_000).is_multiple_of(900_000_000_000));
        }
    }

    #[test]
    fn the_attribute_names_every_bound() {
        let range = attribute(r#"duration_ns(min = "PT1H", max = "PT5H", step = "PT15M")"#)
            .expect("a range")
            .expect("the duration form");
        assert_eq!(range.step(), duration("PT15M"));
        assert_eq!(attribute("Email").expect("another form"), None);
        for (written, problem) in [
            (r#"duration_ns(min = "PT1H", max = "PT5H")"#, "all of"),
            (r#"duration_ns(min = "PT1H", min = "PT2H")"#, "given twice"),
            (r#"duration_ns(least = "PT1H")"#, "takes `min`"),
            (
                r#"duration_ns(min = 1, max = "PT5H", step = "PT1H")"#,
                "in quotes",
            ),
            (
                r#"duration_ns(min = "P1Y", max = "P2Y", step = "P1Y")"#,
                "years or months",
            ),
        ] {
            let message = attribute(written).expect_err(written);
            assert!(message.contains(problem), "{written}: {message}");
        }
    }

    #[test]
    fn a_range_round_trips_as_its_written_bounds() {
        let format = MockFormat::DurationNs(
            DurationRange::new(duration("PT1H"), duration("PT5H"), duration("PT15M"))
                .expect("a range"),
        );
        let written = serde_json::to_value(&format).expect("serializes");
        assert_eq!(
            written,
            serde_json::json!({ "DurationNs": { "min": "PT1H", "max": "PT5H", "step": "PT15M" } })
        );
        assert_eq!(
            serde_json::from_value::<MockFormat>(written).expect("deserializes"),
            format
        );
        let uneven =
            serde_json::json!({ "DurationNs": { "min": "PT1H", "max": "PT2H", "step": "PT25M" } });
        assert!(serde_json::from_value::<MockFormat>(uneven).is_err());
    }
}
