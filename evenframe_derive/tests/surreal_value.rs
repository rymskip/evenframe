//! The derive's `SurrealValue` writes each type in the shape evenframe's
//! schema defines for it, and reads back what it wrote, through the database.

use evenframe::Evenframe;
use evenframe::config::ForeignTypeConfig;
use evenframe::prelude::ordered_float::OrderedFloat;
use evenframe::registry::all_configs;
use evenframe::schemasync::dump::tables_surql;
use evenframe::types::ForeignTypeRegistry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use surrealdb::types::{RecordId, SurrealValue, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[serde(rename_all = "camelCase")]
#[surreal(rename_all = "camelCase")]
pub struct Member {
    pub id: RecordId,
    pub display_name: String,
    #[serde(default)]
    pub visits: u32,
    pub nickname: Option<String>,
    pub tier: Tier,
    pub status: Status,
    pub shape: Shape,
    pub amount: Amount,
    pub score: OrderedFloat<f64>,
    pub session: Duration,
    pub address: Address,
    #[serde(skip)]
    pub cache: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Address {
    pub city: String,
    pub zip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub enum Tier {
    Free,
    Paid { seats: u32 },
    Trial(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[serde(tag = "kind")]
pub enum Status {
    Active,
    Suspended { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[serde(tag = "type", content = "data")]
pub enum Shape {
    Dot,
    Circle(f64),
    Pair(u8, u8),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[serde(untagged)]
pub enum Amount {
    Whole(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, evenframe::SurrealValue)]
#[serde(untagged)]
enum Interval {
    Weekly {
        quantity_of_weeks: u32,
        weekdays: Vec<String>,
    },
    Monthly {
        quantity_of_months: u32,
        day: u32,
    },
    Daily {
        quantity_of_days: u32,
    },
    Yearly {
        quantity_of_years: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, evenframe::SurrealValue)]
#[serde(untagged, deny_unknown_fields)]
enum StrictObject {
    Basic { quantity: u32 },
    Detailed { quantity: u32, label: String },
}

#[test]
fn untagged_objects_round_trip_by_their_required_fields() {
    for interval in [
        Interval::Weekly {
            quantity_of_weeks: 2,
            weekdays: vec!["Monday".to_string(), "Friday".to_string()],
        },
        Interval::Monthly {
            quantity_of_months: 3,
            day: 12,
        },
        Interval::Daily {
            quantity_of_days: 4,
        },
        Interval::Yearly {
            quantity_of_years: 5,
        },
    ] {
        assert_eq!(
            Interval::from_value(interval.clone().into_value()).unwrap(),
            interval
        );
    }
}

#[test]
fn untagged_objects_keep_serdes_variant_order_and_unknown_field_behavior() {
    for value in [
        Value::from_t(BTreeMap::from([
            ("quantity_of_months".to_string(), Value::from_t(2_u32)),
            ("day".to_string(), Value::from_t(12_u32)),
            ("note".to_string(), Value::from_t("ignored".to_string())),
        ])),
        Value::from_t(BTreeMap::from([
            ("quantity_of_weeks".to_string(), Value::from_t(1_u32)),
            (
                "weekdays".to_string(),
                Value::from_t(vec!["Monday".to_string()]),
            ),
            ("quantity_of_months".to_string(), Value::from_t(2_u32)),
            ("day".to_string(), Value::from_t(12_u32)),
        ])),
    ] {
        let expected: Interval = serde_json::from_value(value.clone().into_json_value()).unwrap();
        assert_eq!(Interval::from_value(value).unwrap(), expected);
    }
}

#[test]
fn denying_unknown_fields_distinguishes_subset_object_shapes() {
    for original in [
        StrictObject::Basic { quantity: 2 },
        StrictObject::Detailed {
            quantity: 2,
            label: "full".to_string(),
        },
    ] {
        assert_eq!(
            StrictObject::from_value(original.clone().into_value()).unwrap(),
            original
        );
    }
}

fn member() -> Member {
    Member {
        id: RecordId::new("member", "ada"),
        display_name: "Ada".to_owned(),
        visits: 3,
        nickname: None,
        tier: Tier::Paid { seats: 4 },
        status: Status::Suspended {
            reason: "late".to_owned(),
        },
        shape: Shape::Pair(1, 2),
        amount: Amount::Text("ten".to_owned()),
        score: OrderedFloat(2.5),
        session: Duration::from_millis(1500),
        address: Address {
            city: "Oslo".to_owned(),
            zip: Some("0150".to_owned()),
        },
        cache: String::new(),
    }
}

fn object(value: &Value) -> &surrealdb::types::Object {
    match value {
        Value::Object(object) => object,
        other => panic!("expected an object, got {other:?}"),
    }
}

#[test]
fn fields_take_their_database_names_and_enums_serde_representation() {
    let value = member().into_value();
    let fields = object(&value);
    assert!(fields.contains_key("displayName"));
    assert_eq!(fields.get("nickname"), Some(&Value::None));
    assert!(
        !fields.contains_key("cache"),
        "a skipped field is not written"
    );
    assert!(matches!(fields.get("session"), Some(Value::Duration(_))));
    assert!(matches!(fields.get("id"), Some(Value::RecordId(_))));

    assert_eq!(Tier::Free.into_value(), Value::String("Free".to_owned()));
    let paid = Tier::Paid { seats: 4 }.into_value();
    assert!(object(&paid).contains_key("Paid"));
    let status = Status::Active.into_value();
    assert_eq!(
        object(&status).get("kind"),
        Some(&Value::String("Active".to_owned()))
    );
    let shape = Shape::Circle(1.5).into_value();
    assert!(object(&shape).contains_key("data"));
    assert_eq!(Amount::Whole(7).into_value(), Value::from_t(7_i64));
}

#[test]
fn every_representation_reads_back_what_it_wrote() {
    let original = member();
    assert_eq!(
        Member::from_value(original.clone().into_value()).expect("member reads"),
        original
    );
    for tier in [Tier::Free, Tier::Paid { seats: 1 }, Tier::Trial(9)] {
        assert_eq!(
            Tier::from_value(tier.clone().into_value()).expect("tier reads"),
            tier
        );
    }
    for shape in [Shape::Dot, Shape::Circle(0.5), Shape::Pair(3, 4)] {
        assert_eq!(
            Shape::from_value(shape.clone().into_value()).expect("shape reads"),
            shape
        );
    }
    for amount in [Amount::Whole(1), Amount::Text("one".to_owned())] {
        assert_eq!(
            Amount::from_value(amount.clone().into_value()).expect("amount reads"),
            amount
        );
    }
}

#[test]
fn a_missing_field_takes_serde_default_or_is_reported() {
    let mut fields = BTreeMap::new();
    fields.insert("city".to_owned(), Value::String("Oslo".to_owned()));
    fields.insert("zip".to_owned(), Value::Null);
    let address = Address::from_value(Value::Object(fields.into_iter().collect()))
        .expect("a null option reads as None");
    assert_eq!(address.zip, None);

    let error = Address::from_value(Value::Object(
        BTreeMap::<String, Value>::new().into_iter().collect(),
    ))
    .expect_err("city is required");
    assert!(
        error.to_string().contains("missing the field `city`"),
        "{error}"
    );

    let mut written = object(&member().into_value()).clone();
    written.remove("visits");
    let read = Member::from_value(Value::Object(written)).expect("visits defaults");
    assert_eq!(read.visits, 0);
}

/// The SDK's record id and `OrderedFloat`, as a project maps them.
fn foreign_types() -> BTreeMap<String, ForeignTypeConfig> {
    BTreeMap::from([
        (
            "RecordId".to_owned(),
            ForeignTypeConfig {
                rust_type_names: vec!["RecordId".to_owned()],
                surrealdb: "record".to_owned(),
                surrealdb_id_format: Some("record<{table_name}>".to_owned()),
                ..ForeignTypeConfig::default()
            },
        ),
        (
            "OrderedFloat".to_owned(),
            ForeignTypeConfig {
                rust_type_names: vec!["OrderedFloat".to_owned()],
                surrealdb: "float".to_owned(),
                ..ForeignTypeConfig::default()
            },
        ),
    ])
}

#[tokio::test]
async fn a_record_round_trips_through_the_database() {
    let db = Surreal::new::<Mem>(())
        .await
        .expect("the embedded database starts");
    db.use_ns("walk_surreal_value")
        .use_db("walk_surreal_value")
        .await
        .expect("the namespace opens");

    let types = all_configs()
        .into_schemasync()
        .expect("the registered types have a stored form");
    let schema = tables_surql(
        &types.tables,
        &types.objects,
        &types.enums,
        &ForeignTypeRegistry::from_config(&foreign_types()),
        false,
    )
    .expect("the schema dump generates");
    db.query(schema.as_str())
        .await
        .unwrap_or_else(|error| panic!("the schema runs: {error}\n{schema}"))
        .check()
        .expect("every DEFINE succeeds");

    let original = member();
    db.query("CREATE member:ada CONTENT $record")
        .bind(("record", original.clone().into_value()))
        .await
        .expect("the create runs")
        .check()
        .expect("the record matches the schema");

    let mut response = db
        .query("SELECT * FROM member")
        .await
        .expect("the select runs");
    let read: Vec<Member> = response.take(0).expect("the record reads back");
    assert_eq!(read, vec![original]);
}

/// A query's row: no metadata, serde's attributes read without serde's derive.
#[derive(Debug, PartialEq, evenframe::SurrealValue)]
#[serde(rename_all = "camelCase")]
struct VisitRow {
    member_name: String,
    #[serde(default, alias = "visitCount")]
    visits: u32,
    tier: Plan,
}

#[derive(Debug, PartialEq, evenframe::SurrealValue)]
enum Plan {
    Basic,
    #[serde(alias = "Gold")]
    Premium,
}

#[test]
fn a_row_projection_reads_by_its_serde_attributes() {
    let row = Value::Object(
        BTreeMap::from([
            ("memberName".to_owned(), Value::String("Ada".to_owned())),
            ("tier".to_owned(), Value::String("Premium".to_owned())),
        ])
        .into_iter()
        .collect(),
    );
    assert_eq!(
        VisitRow::from_value(row).expect("the row reads"),
        VisitRow {
            member_name: "Ada".to_owned(),
            visits: 0,
            tier: Plan::Premium,
        }
    );
    assert_eq!(Plan::Basic.into_value(), Value::String("Basic".to_owned()));
}

#[test]
fn a_row_reads_a_field_and_a_variant_by_their_serde_aliases() {
    let row = Value::Object(
        BTreeMap::from([
            ("memberName".to_owned(), Value::String("Ada".to_owned())),
            ("visitCount".to_owned(), 3_i64.into_value()),
            ("tier".to_owned(), Value::String("Gold".to_owned())),
        ])
        .into_iter()
        .collect(),
    );
    assert_eq!(
        VisitRow::from_value(row).expect("the row reads by its aliases"),
        VisitRow {
            member_name: "Ada".to_owned(),
            visits: 3,
            tier: Plan::Premium,
        }
    );
}

#[derive(Debug, PartialEq, evenframe::SurrealValue)]
struct Joined {
    #[serde(flatten)]
    visit: VisitRow,
    owned: bool,
}

#[test]
fn a_flattened_row_reads_the_keys_beside_its_own() {
    let joined = Joined {
        visit: VisitRow {
            member_name: "Ada".to_owned(),
            visits: 2,
            tier: Plan::Basic,
        },
        owned: true,
    };
    let value = Joined::from_value(
        Joined {
            visit: VisitRow {
                member_name: "Ada".to_owned(),
                visits: 2,
                tier: Plan::Basic,
            },
            owned: true,
        }
        .into_value(),
    )
    .expect("the joined row reads");
    assert_eq!(value, joined);
}

#[derive(Debug, PartialEq, evenframe::SurrealValue)]
struct LinkRow {
    owner: String,
    members: Vec<String>,
    note: Option<String>,
}

#[test]
fn a_row_reads_a_record_id_into_text_as_its_json_did() {
    let row = Value::Object(
        BTreeMap::from([
            (
                "owner".to_owned(),
                Value::RecordId(RecordId::new("user", "ada")),
            ),
            (
                "members".to_owned(),
                Value::Array(
                    vec![Value::RecordId(RecordId::new("user", "bo"))]
                        .into_iter()
                        .collect(),
                ),
            ),
            ("note".to_owned(), Value::String("hi".to_owned())),
        ])
        .into_iter()
        .collect(),
    );
    assert_eq!(
        LinkRow::from_value(row).expect("the row reads"),
        LinkRow {
            owner: "user:ada".to_owned(),
            members: vec!["user:bo".to_owned()],
            note: Some("hi".to_owned()),
        }
    );
}

#[derive(Debug, PartialEq, evenframe::SurrealValue)]
struct Labels(std::collections::HashMap<String, String>);

#[derive(Debug, PartialEq, evenframe::SurrealValue)]
struct Span(u32, u32);

#[test]
fn tuple_structs_read_as_serde_writes_them() {
    let labels = Labels(std::collections::HashMap::from([(
        "a".to_owned(),
        "b".to_owned(),
    )]));
    assert!(matches!(
        Labels(labels.0.clone()).into_value(),
        Value::Object(_)
    ));
    assert_eq!(
        Labels::from_value(Labels(labels.0.clone()).into_value()).expect("reads"),
        labels
    );
    assert_eq!(
        Span::from_value(Span(1, 2).into_value()).expect("reads"),
        Span(1, 2)
    );
}

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[serde(deny_unknown_fields)]
struct Signup {
    #[validators(StringValidator::Trim, StringValidator::Lower, StringValidator::Email)]
    email: String,
    #[validators(
        StringValidator::IntegerParse,
        NumberValidator::GreaterThanOrEqualTo(13.0)
    )]
    age: i64,
    #[validators(StringValidator::Capitalize)]
    nickname: Option<String>,
}

#[test]
fn a_stored_read_parses_transforms_and_validates() {
    let signup = Signup::from_value(Value::Object(
        BTreeMap::from([
            (
                "email".to_owned(),
                Value::String("  Ada@Example.COM ".to_owned()),
            ),
            ("age".to_owned(), Value::String("42".to_owned())),
            ("nickname".to_owned(), Value::String("ada".to_owned())),
        ])
        .into_iter()
        .collect(),
    ))
    .expect("the signup reads");
    assert_eq!(signup.email, "ada@example.com");
    assert_eq!(signup.age, 42);
    assert_eq!(signup.nickname.as_deref(), Some("Ada"));

    let error = Signup::from_value(Value::Object(
        BTreeMap::from([
            (
                "email".to_owned(),
                Value::String("ada@example.com".to_owned()),
            ),
            ("age".to_owned(), Value::String("12".to_owned())),
        ])
        .into_iter()
        .collect(),
    ))
    .expect_err("age is below the minimum");
    assert!(error.to_string().contains("age"), "{error}");
}

#[test]
fn unknown_fields_are_rejected_when_serde_denies_them() {
    let error = Signup::from_value(Value::Object(
        BTreeMap::from([
            (
                "email".to_owned(),
                Value::String("ada@example.com".to_owned()),
            ),
            ("age".to_owned(), 20_i64.into_value()),
            ("extra".to_owned(), Value::String("no".to_owned())),
        ])
        .into_iter()
        .collect(),
    ))
    .expect_err("extra is not a field");
    assert!(error.to_string().contains("extra"), "{error}");
}

#[test]
fn kind_of_matches_the_stored_shape() {
    assert!(matches!(Member::kind_of(), surrealdb::types::Kind::Object));
    assert!(matches!(Tier::kind_of(), surrealdb::types::Kind::Either(_)));
    assert!(matches!(
        Amount::kind_of(),
        surrealdb::types::Kind::Either(_)
    ));
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Login {
    pub id: String,
    #[serde(rename(serialize = "userName", deserialize = "user_name"))]
    pub user_name: String,
    #[serde(skip_serializing, default)]
    pub password: String,
    #[serde(skip_deserializing)]
    pub created: String,
}

#[test]
fn a_field_serde_skips_one_way_is_stored_as_serde_writes_it() {
    let login: Login = serde_json::from_str(
        r#"{ "id": "login:ada", "user_name": "ada", "password": "pw", "created": "now" }"#,
    )
    .expect("serde reads the deserialize name");
    assert_eq!(login.user_name, "ada");
    assert_eq!(login.password, "pw");
    assert_eq!(login.created, "", "serde never reads `created`");

    let json = serde_json::to_value(&login).expect("serde writes it");
    assert!(json.get("userName").is_some(), "{json}");
    assert!(json.get("password").is_none(), "{json}");

    let written = Login {
        created: "now".to_owned(),
        ..login
    }
    .into_value();
    let fields = object(&written);
    assert!(fields.contains_key("user_name"));
    assert!(!fields.contains_key("password"), "serde never writes it");
    assert!(fields.contains_key("created"));
    let read = Login::from_value(written).expect("a record reads back");
    assert_eq!(read.password, "", "a key never stored takes its default");
    assert_eq!(
        read.created, "",
        "a key serde never reads takes its default"
    );

    let types = all_configs()
        .into_schemasync()
        .expect("the registered types have a stored form");
    let schema = tables_surql(
        &types.tables,
        &types.objects,
        &types.enums,
        &ForeignTypeRegistry::from_config(&foreign_types()),
        false,
    )
    .expect("the schema dump generates");
    assert!(
        schema.contains("DEFINE FIELD OVERWRITE created ON TABLE login"),
        "{schema}"
    );
    assert!(!schema.contains("password"), "{schema}");
}

/// Tagged variants first, then the variants serde writes bare.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub enum Contact {
    Phone {
        number: String,
    },
    Nobody,
    #[serde(untagged)]
    Email(String),
    #[serde(untagged)]
    Postal {
        street: String,
        city: String,
    },
}

#[test]
fn a_partly_untagged_enum_writes_untagged_variants_bare_and_reads_tagged_first() {
    let contacts = [
        Contact::Phone {
            number: "555".to_owned(),
        },
        Contact::Nobody,
        Contact::Email("ada@example.com".to_owned()),
        Contact::Postal {
            street: "Main".to_owned(),
            city: "Oslo".to_owned(),
        },
    ];
    for contact in &contacts {
        let json = serde_json::to_value(contact).expect("serde writes it");
        let read: Contact = serde_json::from_value(json).expect("serde reads it back");
        assert_eq!(&read, contact);
        assert_eq!(
            &Contact::from_value(contact.clone().into_value()).expect("a record reads back"),
            contact
        );
    }
    assert_eq!(
        Contact::Email("ada@example.com".to_owned()).into_value(),
        Value::String("ada@example.com".to_owned())
    );
    assert_eq!(
        Contact::Nobody.into_value(),
        Value::String("Nobody".to_owned())
    );
    let phone = Contact::Phone {
        number: "555".to_owned(),
    }
    .into_value();
    assert!(object(&phone).contains_key("Phone"));
    let postal = Contact::Postal {
        street: "Main".to_owned(),
        city: "Oslo".to_owned(),
    }
    .into_value();
    assert!(object(&postal).contains_key("street"));

    // A tagged form that fails is reported with every untagged attempt.
    let error = Contact::from_value(Value::from_t(7_i64)).expect_err("no variant reads 7");
    assert!(
        error.to_string().contains("matches none of its variants"),
        "{error}"
    );
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Stamp {
    pub by: String,
}

/// A struct, an Option of one and a map written beside the struct's own keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Note {
    pub text: String,
    #[serde(flatten)]
    pub stamp: Stamp,
    #[serde(flatten)]
    pub place: Option<Address>,
    #[serde(flatten)]
    pub tags: BTreeMap<String, String>,
}

#[test]
fn a_flattened_field_is_stored_beside_its_siblings_as_serde_writes_it() {
    let note = Note {
        text: "hi".to_owned(),
        stamp: Stamp {
            by: "ada".to_owned(),
        },
        place: Some(Address {
            city: "Oslo".to_owned(),
            zip: None,
        }),
        tags: BTreeMap::from([("mood".to_owned(), "calm".to_owned())]),
    };
    let json = serde_json::to_value(&note).expect("serde writes it");
    assert_eq!(json["by"], "ada");
    assert_eq!(json["city"], "Oslo");
    assert_eq!(json["mood"], "calm");
    assert_eq!(
        serde_json::from_value::<Note>(json).expect("serde reads it back"),
        note
    );

    let written = note.clone().into_value();
    let fields = object(&written);
    assert!(fields.contains_key("by"));
    assert!(fields.contains_key("city"));
    assert!(fields.contains_key("mood"));
    // The map reads only the keys the flattened structs before it left.
    assert_eq!(
        Note::from_value(written).expect("a record reads back"),
        note
    );

    let unplaced = Note {
        place: None,
        ..note
    };
    let written = unplaced.clone().into_value();
    assert!(
        !object(&written).contains_key("city"),
        "an absent Option adds no keys"
    );
    assert_eq!(
        Note::from_value(written).expect("an absent Option reads back"),
        unplaced
    );
}
