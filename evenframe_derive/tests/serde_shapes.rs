//! Validators on the shapes serde writes besides an object of named fields:
//! a struct read by conversion, serde's remote handoff, tuple structs and
//! tuple variants, each element checked at its position.

use evenframe::Evenframe;
use evenframe::validator::validate::Validate;
use serde::{Deserialize, Serialize};

fn read<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|error| error.to_string())
}

/// Written and read as a string, its field checked once converted.
#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[serde(into = "String", try_from = "String")]
pub struct Slug {
    #[validators(
        StringValidator::Trim,
        StringValidator::Lower,
        StringValidator::NonEmpty
    )]
    value: String,
}

impl From<Slug> for String {
    fn from(slug: Slug) -> Self {
        slug.value
    }
}

impl TryFrom<String> for Slug {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.contains('/') {
            return Err("a slug has no slashes".to_owned());
        }
        Ok(Self { value })
    }
}

#[test]
fn a_struct_read_by_conversion_checks_the_converted_fields() {
    assert_eq!(
        read::<Slug>(r#"" Hello ""#),
        Ok(Slug {
            value: "hello".to_owned()
        })
    );
    let error = read::<Slug>(r#""   ""#).unwrap_err();
    assert!(
        error.contains("value: must be a non-empty string"),
        "{error}"
    );
    let error = read::<Slug>(r#""a/b""#).unwrap_err();
    assert!(error.contains("a slug has no slashes"), "{error}");
    assert_eq!(
        serde_json::to_string(&Slug {
            value: "hi".to_owned()
        })
        .unwrap(),
        r#""hi""#
    );
}

mod elsewhere {
    /// A type this crate does not own.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Span {
        pub start: u32,
        pub end: u32,
    }
}

#[derive(Serialize, Evenframe)]
#[serde(remote = "elsewhere::Span")]
pub struct SpanDef {
    #[validators(NumberValidator::LessThan(100.0))]
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    #[serde(with = "SpanDef")]
    pub span: elsewhere::Span,
}

#[test]
fn a_remote_definition_hands_serde_its_checked_remote_value() {
    assert_eq!(
        read::<Selection>(r#"{ "span": { "start": 1, "end": 2 } }"#),
        Ok(Selection {
            span: elsewhere::Span { start: 1, end: 2 }
        })
    );
    let error = read::<Selection>(r#"{ "span": { "start": 200, "end": 2 } }"#).unwrap_err();
    assert!(error.contains("start: must be less than 100"), "{error}");
}

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
pub struct Range(
    #[validators(NumberValidator::GreaterThanOrEqualTo(0.0))] i32,
    #[validators(StringValidator::Trim)] String,
);

#[test]
fn a_tuple_struct_checks_each_element_at_its_position() {
    assert_eq!(read::<Range>(r#"[1, " a "]"#), Ok(Range(1, "a".to_owned())));
    let error = read::<Range>(r#"[-1, "a"]"#).unwrap_err();
    assert!(error.contains("0: must be at least 0"), "{error}");
    let error = Range(-1, "a".to_owned()).validate().unwrap_err();
    assert_eq!(error.errors()[0].path, "0");
}

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
pub enum Shape {
    Circle(#[validators(NumberValidator::Positive)] f64),
    Tag(
        #[validators(StringValidator::NonEmpty)] String,
        #[validators(NumberValidator::LessThan(10.0))] u8,
    ),
    Dot,
}

#[test]
fn a_tuple_variant_checks_each_element_at_its_position() {
    assert_eq!(
        read::<Shape>(r#"{ "Circle": 1.5 }"#),
        Ok(Shape::Circle(1.5))
    );
    let error = read::<Shape>(r#"{ "Circle": -1.0 }"#).unwrap_err();
    assert!(error.contains("Circle.0: must be positive"), "{error}");
    let error = read::<Shape>(r#"{ "Tag": ["", 20] }"#).unwrap_err();
    assert!(
        error.contains("Tag.0: must be a non-empty string"),
        "{error}"
    );
    assert!(error.contains("Tag.1: must be less than 10"), "{error}");
    assert_eq!(read::<Shape>(r#""Dot""#), Ok(Shape::Dot));
}

/// Read from a number, its payload checked once converted.
#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[serde(try_from = "u8")]
pub enum Level {
    Off,
    On(#[validators(NumberValidator::LessThan(10.0))] u8),
}

impl TryFrom<u8> for Level {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Off,
            level => Self::On(level),
        })
    }
}

#[test]
fn an_enum_read_by_conversion_checks_the_converted_variant() {
    assert_eq!(read::<Level>("0"), Ok(Level::Off));
    assert_eq!(read::<Level>("3"), Ok(Level::On(3)));
    let error = read::<Level>("50").unwrap_err();
    assert!(error.contains("On.0: must be less than 10"), "{error}");
}
