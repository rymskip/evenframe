//! Deserializing through the derive applies each field's validators: checks
//! reject, morphs transform, and parse morphs read a string into the field's
//! type.

use chrono::{DateTime, Utc};
use evenframe::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Signup {
    #[validators(StringValidator::Trim, StringValidator::Lower, StringValidator::Email)]
    pub email: String,
    #[validators(
        StringValidator::IntegerParse,
        NumberValidator::GreaterThanOrEqualTo(13.0)
    )]
    pub age: i64,
    #[validators(StringValidator::MinLength(2), StringValidator::Capitalize)]
    pub nickname: Option<String>,
    #[validators(StringValidator::DateIsoParse)]
    pub joined: DateTime<Utc>,
    #[validators(ArrayValidator::MaxItems(2))]
    pub tags: Vec<String>,
}

fn signup(fields: serde_json::Value) -> Result<Signup, serde_json::Error> {
    let mut document = serde_json::json!({
        "email": "  Someone@Example.COM ",
        "age": "42",
        "nickname": "bo",
        "joined": "2024-02-29T10:15:00Z",
        "tags": ["a"],
    });
    if let (Some(document), Some(fields)) = (document.as_object_mut(), fields.as_object()) {
        document.extend(fields.clone());
    }
    serde_json::from_value(document)
}

#[test]
fn morphs_transform_and_parses_produce_the_field_type() {
    let signup = signup(serde_json::json!({})).unwrap();
    assert_eq!(signup.email, "someone@example.com");
    assert_eq!(signup.age, 42);
    assert_eq!(signup.nickname.as_deref(), Some("Bo"));
    assert_eq!(signup.joined.to_rfc3339(), "2024-02-29T10:15:00+00:00");
}

#[test]
fn checks_reject_with_the_field_name() {
    let error = signup(serde_json::json!({ "email": "not an email" })).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("email: must be an email address"),
        "{error}"
    );

    let error = signup(serde_json::json!({ "age": "12" })).unwrap_err();
    assert!(
        error.to_string().starts_with("age: must be at least 13"),
        "{error}"
    );

    let error = signup(serde_json::json!({ "age": "4.2" })).unwrap_err();
    assert!(
        error.to_string().starts_with("age: must be an integer"),
        "{error}"
    );

    let error = signup(serde_json::json!({ "tags": ["a", "b", "c"] })).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("tags: must be at most 2 items"),
        "{error}"
    );
}

#[test]
fn optional_fields_validate_only_a_present_value() {
    assert_eq!(
        signup(serde_json::json!({ "nickname": null }))
            .unwrap()
            .nickname,
        None
    );
    let error = signup(serde_json::json!({ "nickname": "b" })).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("nickname: must be at least 2 characters"),
        "{error}"
    );
}
