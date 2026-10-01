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

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Timer {
    #[validators(DurationValidator::LessThanOrEqualToDuration("8h"))]
    pub limit: std::time::Duration,
    #[validators(DurationValidator::GreaterThanDuration("1m"))]
    pub grace: Option<std::time::Duration>,
}

#[test]
fn durations_are_read_as_serde_writes_them_and_checked() {
    let timer: Timer = serde_json::from_value(serde_json::json!({
        "limit": { "secs": 5400, "nanos": 0 },
        "grace": null,
    }))
    .unwrap();
    assert_eq!(timer.limit, std::time::Duration::from_secs(5400));
    assert_eq!(
        serde_json::to_value(&timer).unwrap()["limit"],
        serde_json::json!({ "secs": 5400, "nanos": 0 })
    );

    let error = serde_json::from_value::<Timer>(serde_json::json!({
        "limit": { "secs": 9 * 3600, "nanos": 0 },
        "grace": null,
    }))
    .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("limit: must be at most 8h long"),
        "{error}"
    );

    let error = serde_json::from_value::<Timer>(serde_json::json!({
        "limit": { "secs": 60, "nanos": 0 },
        "grace": { "secs": 30, "nanos": 0 },
    }))
    .unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("grace: must be longer than 1m"),
        "{error}"
    );
}
