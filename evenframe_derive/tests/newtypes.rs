//! A struct serde writes as another type: a newtype reads and stores its one
//! field's value, checks it against its validators, and a struct holding one
//! reports that value's failure at the field it sits in. A validator on a
//! field holding a newtype checks the value inside it.

use evenframe::Evenframe;
use evenframe::registry::get_newtype_config;
use evenframe::types::{FieldType, FromText, NewtypeKind};
use evenframe::validator::validate::Validate;
use serde::{Deserialize, Serialize};
use surrealdb::types::{SurrealValue, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[validators(StringValidator::NonEmpty)]
pub struct NonEmptyString(String);

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[morphs(trim, lower)]
pub struct Handle(NonEmptyString);

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[validators(NumberValidator::GreaterThanOrEqualTo(0.0))]
pub struct Count(FromText<i64>);

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[serde(transparent)]
pub struct Label {
    #[validators(StringValidator::MinLength(2))]
    value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Plain(String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Pair(String, u32);

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
pub struct Account {
    #[validators(StringValidator::MaxLength(5))]
    pub name: NonEmptyString,
    #[morphs(trim)]
    pub title: NonEmptyString,
    #[morphs(lower)]
    pub nickname: Option<NonEmptyString>,
    #[validators(NumberValidator::LessThan(100.0))]
    pub visits: Count,
    #[validators(NumberValidator::GreaterThanOrEqualTo(18.0))]
    pub age: FromText<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Profile {
    pub name: NonEmptyString,
    pub nickname: Option<NonEmptyString>,
    pub tags: Vec<NonEmptyString>,
}

fn read<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, String> {
    serde_json::from_str(json).map_err(|error| error.to_string())
}

#[test]
fn a_newtype_reads_its_value_through_its_validators() {
    assert_eq!(
        read::<NonEmptyString>(r#""a""#),
        Ok(NonEmptyString("a".to_owned()))
    );
    let error = read::<NonEmptyString>(r#""""#).unwrap_err();
    assert!(error.starts_with("must be a non-empty string"), "{error}");

    assert_eq!(
        read::<Handle>(r#""  Bob ""#),
        Ok(Handle(NonEmptyString("bob".to_owned())))
    );
    let error = read::<Handle>(r#""   ""#).unwrap_err();
    assert!(error.starts_with("must be a non-empty string"), "{error}");

    assert_eq!(read::<Count>(r#""42""#), Ok(Count(FromText(42))));
    assert!(read::<Count>(r#""-1""#).is_err());
    assert!(read::<Count>(r#""forty""#).is_err());
}

#[test]
fn a_transparent_struct_is_its_one_field() {
    assert_eq!(
        read::<Label>(r#""ab""#),
        Ok(Label {
            value: "ab".to_owned()
        })
    );
    assert!(read::<Label>(r#""a""#).is_err());
    assert_eq!(
        serde_json::to_string(&Label {
            value: "ab".to_owned()
        })
        .unwrap(),
        r#""ab""#
    );
}

#[test]
fn a_newtype_is_built_from_its_value_through_its_validators() {
    assert_eq!(
        NonEmptyString::try_from("a".to_owned()),
        Ok(NonEmptyString("a".to_owned()))
    );
    let error = NonEmptyString::try_from("").unwrap_err();
    assert_eq!(error.to_string(), "must be a non-empty string");
    assert_eq!(
        NonEmptyString::try_from("a").map(|text| text.as_str().to_owned()),
        Ok("a".to_owned())
    );

    // The value's transforms run, then its own validators check the result.
    assert_eq!(
        Handle::try_from(NonEmptyString("  Bob ".to_owned())),
        Ok(Handle(NonEmptyString("bob".to_owned())))
    );
    assert!(Handle::try_from(NonEmptyString("   ".to_owned())).is_err());

    // A text form is built from its value, which is checked as read.
    assert_eq!(Count::try_from(FromText(42)), Ok(Count(FromText(42))));
    assert!(Count::try_from(FromText(-1)).is_err());

    assert_eq!(
        Label::try_from("ab").map(|label| label.as_str().to_owned()),
        Ok("ab".to_owned())
    );
    assert!(Label::try_from("a").is_err());
}

#[test]
fn a_struct_holding_newtypes_reports_their_failures_at_its_fields() {
    let error = read::<Profile>(r#"{ "name": "", "nickname": null, "tags": [] }"#).unwrap_err();
    assert!(error.contains("must be a non-empty string"), "{error}");

    let profile = Profile {
        name: NonEmptyString(String::new()),
        nickname: Some(NonEmptyString(String::new())),
        tags: vec![
            NonEmptyString("a".to_owned()),
            NonEmptyString(String::new()),
        ],
    };
    let error = profile.validate().unwrap_err();
    let paths: Vec<&str> = error
        .errors()
        .iter()
        .map(|failure| failure.path.as_str())
        .collect();
    assert_eq!(paths, ["name", "nickname", "tags[1]"], "{error}");
    assert_eq!(
        error.to_string(),
        "name: must be a non-empty string; nickname: must be a non-empty string; \
         tags[1]: must be a non-empty string"
    );
}

#[test]
fn a_newtype_is_stored_as_its_value_and_checked_on_read() {
    assert_eq!(
        NonEmptyString("a".to_owned()).into_value(),
        Value::String("a".to_owned())
    );
    assert_eq!(
        NonEmptyString::from_value(Value::String("a".to_owned())).unwrap(),
        NonEmptyString("a".to_owned())
    );
    assert!(NonEmptyString::from_value(Value::String(String::new())).is_err());
    assert_eq!(
        Label {
            value: "ab".to_owned()
        }
        .into_value(),
        Value::String("ab".to_owned())
    );
}

#[test]
fn newtypes_without_validators_and_tuple_structs_keep_serdes_shape() {
    assert_eq!(read::<Plain>(r#""""#), Ok(Plain(String::new())));
    assert_eq!(read::<Pair>(r#"["a", 2]"#), Ok(Pair("a".to_owned(), 2)));
}

#[test]
fn the_registry_sees_a_newtype_as_the_value_it_holds() {
    use evenframe::registry::underlying;
    assert_eq!(
        underlying(&FieldType::Other("Handle".to_owned())),
        &FieldType::String,
        "Handle holds a NonEmptyString, which holds a String"
    );
    let plain = FieldType::Other("NotANewtype".to_owned());
    assert_eq!(underlying(&plain), &plain);
}

#[test]
fn a_newtype_registers_what_it_is_written_as() {
    let config = get_newtype_config("NonEmptyString").expect("NonEmptyString is registered");
    assert_eq!(config.inner, FieldType::String);
    assert_eq!(config.kind, NewtypeKind::Branded);
    assert_eq!(config.validators.len(), 1);

    let pair = get_newtype_config("Pair").expect("Pair is registered");
    assert_eq!(pair.kind, NewtypeKind::Alias);
    assert_eq!(
        pair.inner,
        FieldType::Tuple(vec![FieldType::String, FieldType::U32])
    );
}

fn account(name: &str, title: &str, visits: &str, age: &str) -> String {
    format!(
        r#"{{ "name": "{name}", "title": "{title}", "nickname": "Ada", "visits": "{visits}", "age": "{age}" }}"#
    )
}

#[test]
fn a_field_validator_checks_the_value_a_newtype_holds() {
    assert_eq!(
        read::<Account>(&account("Ada", " Lead ", "3", "30")),
        Ok(Account {
            name: NonEmptyString("Ada".to_owned()),
            title: NonEmptyString("Lead".to_owned()),
            nickname: Some(NonEmptyString("ada".to_owned())),
            visits: Count(FromText(3)),
            age: FromText(30),
        })
    );

    let error = read::<Account>(&account("Adalyn", "Lead", "300", "12")).unwrap_err();
    assert!(error.contains("name: must be"), "{error}");
    assert!(error.contains("visits: must be less than 100"), "{error}");
    assert!(error.contains("age: must be at least 18"), "{error}");

    let error = read::<Account>(&account("Ada", "   ", "3", "30")).unwrap_err();
    assert!(
        error.contains("title: must be a non-empty string"),
        "{error}"
    );

    let account = Account {
        name: NonEmptyString("Adalyn".to_owned()),
        title: NonEmptyString("Lead".to_owned()),
        nickname: None,
        visits: Count(FromText(3)),
        age: FromText(17),
    };
    let error = account.validate().unwrap_err();
    let paths: Vec<&str> = error
        .errors()
        .iter()
        .map(|failure| failure.path.as_str())
        .collect();
    assert_eq!(paths, ["name", "age"], "{error}");
}
