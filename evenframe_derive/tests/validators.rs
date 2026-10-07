//! Deserializing through the derive applies each field's morphs and
//! validators: morphs rewrite, checks reject, and a text-form type reads its
//! value from text.

use evenframe::Evenframe;
use evenframe::types::{FromText, IsoDate};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Signup {
    #[morphs(trim, lower)]
    #[validators(StringValidator::Email)]
    pub email: String,
    #[validators(NumberValidator::GreaterThanOrEqualTo(13.0))]
    pub age: FromText<i64>,
    #[morphs(capitalize)]
    #[validators(StringValidator::MinLength(2))]
    pub nickname: Option<String>,
    pub joined: IsoDate,
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
fn morphs_rewrite_and_text_forms_read_their_value() {
    let signup = signup(serde_json::json!({})).unwrap();
    assert_eq!(signup.email, "someone@example.com");
    assert_eq!(signup.age, FromText(42));
    assert_eq!(signup.nickname.as_deref(), Some("Bo"));
    assert_eq!(signup.joined.0.to_rfc3339(), "2024-02-29T10:15:00+00:00");
    assert_eq!(
        serde_json::to_value(&signup).unwrap()["joined"],
        "2024-02-29T10:15:00.000Z"
    );
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
        error.to_string().starts_with("must be an integer"),
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

#[test]
fn every_failing_field_is_reported_together() {
    let error =
        signup(serde_json::json!({ "email": "nope", "age": "12", "tags": [] })).unwrap_err();
    let message = error.to_string();
    assert!(
        message.starts_with("email: must be an email address; age: must be at least 13"),
        "{message}"
    );
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[validators(StringValidator::MinLength(2))]
    #[serde(alias = "name")]
    pub display_name: String,
    #[validators(StringValidator::Email)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub follower_count: u32,
}

#[test]
fn fields_are_read_by_serde_names_and_aliases() {
    let profile: Profile = serde_json::from_value(serde_json::json!({
        "displayName": "Ada",
        "contactEmail": "ada@example.com",
        "followerCount": 3,
        "unknown": true,
    }))
    .unwrap();
    assert_eq!(profile.display_name, "Ada");
    assert_eq!(profile.follower_count, 3);

    let profile: Profile = serde_json::from_value(serde_json::json!({ "name": "Ada" })).unwrap();
    assert_eq!(profile.contact_email, None);
    assert_eq!(profile.follower_count, 0);

    let error =
        serde_json::from_value::<Profile>(serde_json::json!({ "displayName": "A" })).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("displayName: must be at least 2 characters"),
        "{error}"
    );
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[serde(deny_unknown_fields)]
pub struct Strict {
    #[validators(StringValidator::NonEmpty)]
    pub name: String,
}

#[test]
fn unknown_keys_are_rejected_only_under_deny_unknown_fields() {
    let error = serde_json::from_value::<Strict>(serde_json::json!({ "name": "a", "extra": 1 }))
        .unwrap_err();
    assert!(
        error.to_string().contains("unknown field `extra`"),
        "{error}"
    );
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Team {
    #[validators(StringValidator::NonEmpty)]
    pub name: String,
    pub lead: Profile,
    pub members: Vec<Profile>,
}

#[test]
fn validate_checks_a_built_value_and_everything_it_holds() {
    use evenframe::validator::validate::Validate;

    let profile = |name: &str| Profile {
        display_name: name.to_owned(),
        contact_email: None,
        follower_count: 0,
    };
    let team = Team {
        name: String::new(),
        lead: profile("A"),
        members: vec![profile("Bea"), profile("C")],
    };
    let error = team.validate().unwrap_err();
    let paths: Vec<&str> = error
        .errors()
        .iter()
        .map(|failure| failure.path.as_str())
        .collect();
    assert_eq!(
        paths,
        ["name", "lead.displayName", "members[1].displayName"]
    );

    let valid = Team {
        name: "Core".to_owned(),
        lead: profile("Ada"),
        members: vec![],
    };
    assert!(valid.validate().is_ok());
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[serde(tag = "variant")]
pub enum Contact {
    Company {
        #[morphs(trim)]
        #[validators(StringValidator::NonEmpty)]
        company_name: String,
    },
    Person {
        #[validators(StringValidator::NonEmpty)]
        first_name: String,
        #[validators(StringValidator::MinLength(2))]
        nickname: Option<String>,
    },
    Unknown,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Account {
    #[validators(StringValidator::NonEmpty)]
    pub handle: String,
    pub contact: Contact,
}

#[test]
fn enum_variant_fields_are_read_through_their_validators() {
    let company = serde_json::from_value::<Contact>(serde_json::json!({
        "variant": "Company",
        "company_name": "  Acme  ",
    }))
    .unwrap();
    assert!(
        matches!(&company, Contact::Company { company_name } if company_name == "Acme"),
        "{company:?}"
    );

    let blank = serde_json::from_value::<Contact>(serde_json::json!({
        "variant": "Company",
        "company_name": "   ",
    }))
    .unwrap_err();
    assert!(
        blank.to_string().starts_with("Company.company_name: "),
        "{blank}"
    );

    let nested = serde_json::from_value::<Account>(serde_json::json!({
        "handle": "acme",
        "contact": { "variant": "Person", "first_name": "", "nickname": "x" },
    }))
    .unwrap_err();
    assert!(nested.to_string().contains("Person.first_name"), "{nested}");

    let unknown = serde_json::from_value::<Contact>(serde_json::json!({ "variant": "Unknown" }));
    assert!(matches!(unknown, Ok(Contact::Unknown)), "{unknown:?}");
}

#[test]
fn validate_checks_a_built_enum_variant() {
    use evenframe::validator::validate::Validate;

    let person = Contact::Person {
        first_name: String::new(),
        nickname: Some("x".to_owned()),
    };
    let paths: Vec<String> = person
        .validate()
        .unwrap_err()
        .errors()
        .iter()
        .map(|failure| failure.path.clone())
        .collect();
    assert_eq!(paths, ["Person.first_name", "Person.nickname"]);

    let named = Contact::Person {
        first_name: "Ada".to_owned(),
        nickname: None,
    };
    assert!(named.validate().is_ok());
    assert!(Contact::Unknown.validate().is_ok());
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Reading {
    #[morphs(collapse_whitespace, trim)]
    pub label: String,
    #[morphs(clamp = ("0", "10"), round = 1)]
    pub level: f64,
    #[morphs(clamp = ("1", "5"))]
    pub rating: u8,
    #[morphs(unique, sort)]
    pub tags: Vec<String>,
    #[morphs(clamp = ("0", "99.5"))]
    pub price: String,
}

#[test]
fn every_morph_family_rewrites_the_value_it_applies_to() {
    let reading: Reading = serde_json::from_value(serde_json::json!({
        "label": "  low \t  battery ",
        "level": 12.345,
        "rating": 9,
        "tags": ["b", "a", "b"],
        "price": "120.25",
    }))
    .unwrap();
    assert_eq!(reading.label, "low battery");
    assert_eq!(reading.level, 10.0);
    assert_eq!(reading.rating, 5);
    assert_eq!(reading.tags, vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(reading.price, "99.5");

    let rounded: Reading = serde_json::from_value(serde_json::json!({
        "label": "x",
        "level": 2.25,
        "rating": 0,
        "tags": [],
        "price": "1",
    }))
    .unwrap();
    assert_eq!(rounded.level, 2.3);
    assert_eq!(rounded.rating, 1);
}
