//! Each `#[surreal]` key overrides how the database stores an item, an item
//! serde skips is stored only where `#[surreal]` or `#[schemasync(retain)]`
//! keeps it, and the schema defines what `SurrealValue` writes.

use evenframe::Evenframe;
use evenframe::config::ForeignTypeConfig;
use evenframe::registry::all_configs;
use evenframe::schemasync::dump::tables_surql;
use evenframe::types::ForeignTypeRegistry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use surrealdb::types::{Number, RecordId, SurrealValue, Value};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Stamp {
    pub by: String,
}

/// Tagged externally by serde and internally in the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[surreal(tag = "kind")]
pub enum Tone {
    Loud { level: u8 },
    Quiet,
}

/// Field keys, and the fields serde skips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Account {
    pub id: RecordId,
    #[surreal(rename = "full_name")]
    pub name: String,
    #[surreal(default)]
    pub visits: u32,
    #[surreal(skip)]
    #[serde(default)]
    pub session: String,
    #[serde(skip)]
    #[schemasync(retain)]
    pub cache: String,
    #[serde(skip)]
    pub scratch: String,
    #[surreal(wrap)]
    pub tone: Tone,
    #[surreal(flatten)]
    pub stamp: Stamp,
    #[serde(with = "upper")]
    pub code: String,
}

/// Written uppercase and read lowercase.
mod upper {
    pub fn serialize<Serializer: serde::Serializer>(
        value: &str,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error> {
        serializer.serialize_str(&value.to_uppercase())
    }

    pub fn deserialize<'de, Deserializer: serde::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> Result<String, Deserializer::Error> {
        let written: String = serde::Deserialize::deserialize(deserializer)?;
        Ok(written.to_lowercase())
    }
}

fn account() -> Account {
    Account {
        id: RecordId::new("account", "ada"),
        name: "Ada".to_owned(),
        visits: 3,
        session: "s-1".to_owned(),
        cache: "warm".to_owned(),
        scratch: "notes".to_owned(),
        tone: Tone::Loud { level: 3 },
        stamp: Stamp {
            by: "admin".to_owned(),
        },
        code: "abc".to_owned(),
    }
}

fn object(value: &Value) -> &surrealdb::types::Object {
    match value {
        Value::Object(object) => object,
        other => panic!("expected an object, got {other:?}"),
    }
}

#[test]
fn field_keys_override_serde_and_skipped_fields_are_kept_only_when_retained() {
    let json = serde_json::to_value(account()).expect("serde writes it");
    assert_eq!(json["name"], "Ada", "serde keeps its own name");
    assert_eq!(json["stamp"]["by"], "admin", "serde nests the struct");
    assert_eq!(json["session"], "s-1");

    let written = account().into_value();
    let fields = object(&written);
    assert!(fields.contains_key("full_name"));
    assert!(!fields.contains_key("name"));
    assert!(
        !fields.contains_key("session"),
        "#[surreal(skip)] is not stored"
    );
    assert_eq!(
        fields.get("cache"),
        Some(&Value::String("warm".to_owned())),
        "#[schemasync(retain)] stores what serde skips"
    );
    assert!(!fields.contains_key("scratch"), "serde skips it");
    assert!(
        object(fields.get("tone").expect("the tone is stored")).contains_key("Loud"),
        "#[surreal(wrap)] stores serde's shape, not the tagged one: {fields:?}"
    );
    assert_eq!(fields.get("by"), Some(&Value::String("admin".to_owned())));
    assert!(
        !fields.contains_key("stamp"),
        "#[surreal(flatten)] stores it beside"
    );
    assert_eq!(
        fields.get("code"),
        Some(&Value::String("ABC".to_owned())),
        "#[serde(with)] stores what the module writes"
    );

    let read = Account::from_value(written.clone()).expect("the record reads back");
    assert_eq!(
        read,
        Account {
            session: String::new(),
            scratch: String::new(),
            ..account()
        }
    );

    let mut without_visits = fields.clone();
    without_visits.remove("visits");
    let read = Account::from_value(Value::Object(without_visits)).expect("visits defaults");
    assert_eq!(read.visits, 0, "#[surreal(default)] fills a missing key");
}

/// A container's `#[schemasync(retain)]` keeps every field serde skips, and
/// its `#[surreal]` keys name them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[schemasync(retain)]
#[surreal(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub body_text: String,
    #[serde(skip)]
    pub local_note: String,
}

#[test]
fn a_retained_container_stores_every_field_serde_skips() {
    let draft = Draft {
        id: "draft:one".to_owned(),
        body_text: "hello".to_owned(),
        local_note: "mine".to_owned(),
    };
    let written = draft.clone().into_value();
    let fields = object(&written);
    assert!(fields.contains_key("bodyText"));
    assert_eq!(
        fields.get("localNote"),
        Some(&Value::String("mine".to_owned()))
    );
    assert_eq!(Draft::from_value(written).expect("it reads back"), draft);
}

/// A variant serde skips is stored as NONE unless retained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub enum Phase {
    Open,
    #[serde(skip)]
    Hidden,
    #[serde(skip)]
    #[schemasync(retain)]
    Archived,
}

#[test]
fn a_skipped_variant_is_stored_as_none_and_a_retained_one_by_name() {
    assert_eq!(Phase::Hidden.into_value(), Value::None);
    let error = Phase::from_value(Value::None).expect_err("NONE reads as no variant");
    assert!(error.to_string().contains("stored as NONE"), "{error}");
    assert_eq!(
        Phase::Archived.into_value(),
        Value::String("Archived".to_owned())
    );
    for phase in [Phase::Open, Phase::Archived] {
        assert_eq!(
            Phase::from_value(phase.clone().into_value()).expect("it reads back"),
            phase
        );
    }
}

/// serde writes an untagged unit variant as null; `value` stores a literal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[serde(untagged)]
pub enum Limit {
    Count(i64),
    Unknown,
    #[surreal(value = "unlimited")]
    Unlimited,
    #[surreal(value = none)]
    Unset,
}

#[test]
fn an_untagged_unit_variant_is_stored_as_null_or_its_value() {
    assert_eq!(
        serde_json::to_value(Limit::Unknown).expect("serde writes it"),
        serde_json::Value::Null
    );
    assert_eq!(Limit::Unknown.into_value(), Value::Null);
    assert_eq!(
        Limit::Unlimited.into_value(),
        Value::String("unlimited".to_owned())
    );
    assert_eq!(Limit::Unset.into_value(), Value::None);
    assert_eq!(Limit::Count(4).into_value(), Value::Number(Number::Int(4)));
    for limit in [
        Limit::Count(4),
        Limit::Unknown,
        Limit::Unlimited,
        Limit::Unset,
    ] {
        assert_eq!(
            Limit::from_value(limit.clone().into_value()).expect("it reads back"),
            limit
        );
    }
}

/// `other` reads any tag no variant names; `tuple` stores a payload as a
/// one-element array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub enum Signal {
    Ping,
    #[surreal(tuple)]
    Level(i64),
    #[surreal(other)]
    Unknown,
}

#[test]
fn other_reads_unknown_tags_and_tuple_stores_an_array() {
    let level = Signal::Level(7).into_value();
    assert_eq!(
        object(&level).get("Level"),
        Some(&Value::Array(
            [Value::Number(Number::Int(7))].into_iter().collect()
        ))
    );
    assert_eq!(
        Signal::from_value(level).expect("it reads back"),
        Signal::Level(7)
    );
    assert_eq!(
        Signal::from_value(Value::String("Pong".to_owned())).expect("other reads it"),
        Signal::Unknown
    );
    assert_eq!(
        Signal::Unknown.into_value(),
        Value::String("Unknown".to_owned())
    );
}

/// `skip_content` never writes the content; `skip_content_if` leaves it out
/// when the predicate holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[serde(tag = "type", content = "data")]
pub enum Event {
    Opened,
    #[surreal(skip_content)]
    Closed(String),
    #[surreal(skip_content_if = "blank")]
    Renamed(String),
}

fn blank(content: &Value) -> bool {
    matches!(content, Value::String(text) if text.is_empty())
}

#[test]
fn skip_content_leaves_out_an_adjacent_variants_content() {
    let closed = Event::Closed("done".to_owned()).into_value();
    assert!(!object(&closed).contains_key("data"));
    assert_eq!(
        Event::from_value(closed).expect("it reads back"),
        Event::Closed(String::new()),
        "content never stored reads as its default"
    );

    let unnamed = Event::Renamed(String::new()).into_value();
    assert!(!object(&unnamed).contains_key("data"));
    let renamed = Event::Renamed("new".to_owned()).into_value();
    assert_eq!(
        object(&renamed).get("data"),
        Some(&Value::String("new".to_owned()))
    );
    for event in [
        Event::Opened,
        Event::Renamed(String::new()),
        Event::Renamed("new".to_owned()),
    ] {
        assert_eq!(
            Event::from_value(event.clone().into_value()).expect("it reads back"),
            event
        );
    }
}

/// An internally tagged newtype variant stores its tag beside the struct's keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[serde(tag = "kind")]
pub enum Pet {
    Dog(Stamp),
    Cat { lives: u8 },
}

#[test]
fn an_internally_tagged_newtype_variant_merges_its_tag() {
    let dog = Pet::Dog(Stamp {
        by: "rex".to_owned(),
    });
    let json = serde_json::to_value(&dog).expect("serde writes it");
    assert_eq!(json["kind"], "Dog");
    assert_eq!(json["by"], "rex");

    let written = dog.clone().into_value();
    let fields = object(&written);
    assert_eq!(fields.get("kind"), Some(&Value::String("Dog".to_owned())));
    assert_eq!(fields.get("by"), Some(&Value::String("rex".to_owned())));
    assert_eq!(Pet::from_value(written).expect("it reads back"), dog);
}

/// serde writes a unit struct as null; `value` stores a literal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Marker;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[surreal(value = "on")]
pub struct Switch;

/// A tuple struct stored as an array, and one whose element is stored through
/// serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
#[surreal(tuple)]
pub struct Boxed(pub i64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Muffled(#[surreal(wrap)] pub Tone);

#[test]
fn a_struct_written_as_another_value_takes_its_own_keys() {
    assert_eq!(Marker.into_value(), Value::Null);
    assert_eq!(Marker::from_value(Value::Null).expect("null reads"), Marker);
    assert_eq!(Switch.into_value(), Value::String("on".to_owned()));
    assert_eq!(
        Switch::from_value(Value::String("on".to_owned())).expect("the value reads"),
        Switch
    );
    assert_eq!(
        Boxed(5).into_value(),
        Value::Array([Value::Number(Number::Int(5))].into_iter().collect())
    );
    assert_eq!(
        Boxed::from_value(Boxed(5).into_value()).expect("it reads back"),
        Boxed(5)
    );
    let tagged = Tone::Loud { level: 1 }.into_value();
    assert_eq!(
        object(&tagged).get("kind"),
        Some(&Value::String("Loud".to_owned()))
    );
    let muffled = Muffled(Tone::Loud { level: 1 });
    assert!(object(&muffled.clone().into_value()).contains_key("Loud"));
    assert_eq!(
        Muffled::from_value(muffled.clone().into_value()).expect("it reads back"),
        muffled
    );
}

/// A record holding every stored shape above, for the schema to define.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Evenframe)]
pub struct Ticket {
    pub id: RecordId,
    pub phase: Phase,
    pub limit: Limit,
    pub signal: Signal,
    pub event: Event,
    pub pet: Pet,
    pub marker: Marker,
    pub switch: Switch,
    pub switches: Vec<Switch>,
    pub boxed: Boxed,
    pub muffled: Muffled,
}

fn ticket() -> Ticket {
    Ticket {
        id: RecordId::new("ticket", "one"),
        phase: Phase::Archived,
        limit: Limit::Unlimited,
        signal: Signal::Level(2),
        event: Event::Closed(String::new()),
        pet: Pet::Dog(Stamp {
            by: "rex".to_owned(),
        }),
        marker: Marker,
        switch: Switch,
        switches: vec![Switch, Switch],
        boxed: Boxed(9),
        muffled: Muffled(Tone::Quiet),
    }
}

fn schema() -> String {
    let types = all_configs()
        .into_schemasync()
        .expect("the registered types have a stored form");
    tables_surql(
        &types,
        &ForeignTypeRegistry::from_config(&BTreeMap::from([(
            "RecordId".to_owned(),
            ForeignTypeConfig {
                rust_type_names: vec!["RecordId".to_owned()],
                surrealdb: "record".to_owned(),
                surrealdb_id_format: Some("record<{table_name}>".to_owned()),
                ..ForeignTypeConfig::default()
            },
        )])),
        false,
    )
    .expect("the schema dump generates")
}

fn field_definition<'a>(schema: &'a str, table: &str, field: &str) -> &'a str {
    let prefix = format!("DEFINE FIELD OVERWRITE {field} ON TABLE {table} ");
    schema
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("`{field}` is defined on `{table}`:\n{schema}"))
}

#[test]
fn the_schema_defines_what_is_stored() {
    let schema = schema();
    for field in ["full_name", "cache", "by", "visits"] {
        field_definition(&schema, "account", field);
    }
    for field in ["session", "scratch", "stamp", "name"] {
        assert!(
            !schema.contains(&format!("DEFINE FIELD OVERWRITE {field} ON TABLE account ")),
            "`{field}` is not stored:\n{schema}"
        );
    }
    assert!(field_definition(&schema, "account", "tone").contains("TYPE any"));
    assert!(field_definition(&schema, "ticket", "muffled").contains("TYPE any"));
    assert!(field_definition(&schema, "account", "code").contains("TYPE any"));
    field_definition(&schema, "draft", "localNote");
    assert!(field_definition(&schema, "ticket", "phase").contains("none"));
    assert!(field_definition(&schema, "ticket", "limit").contains("'unlimited'"));
    assert!(field_definition(&schema, "ticket", "switch").contains("'on'"));
    assert!(field_definition(&schema, "ticket", "switches").contains("'on'"));
}

#[tokio::test]
async fn every_stored_shape_round_trips_through_the_database() {
    let db = Surreal::new::<Mem>(())
        .await
        .expect("the embedded database starts");
    db.use_ns("surreal_keys")
        .use_db("surreal_keys")
        .await
        .expect("the namespace opens");
    let schema = schema();
    db.query(schema.as_str())
        .await
        .unwrap_or_else(|error| panic!("the schema runs: {error}\n{schema}"))
        .check()
        .unwrap_or_else(|error| panic!("every DEFINE succeeds: {error}\n{schema}"));

    let stored_account = Account {
        session: String::new(),
        scratch: String::new(),
        ..account()
    };
    db.query("CREATE account:ada CONTENT $record")
        .bind(("record", stored_account.clone().into_value()))
        .await
        .expect("the create runs")
        .check()
        .unwrap_or_else(|error| panic!("the account matches the schema: {error}\n{schema}"));
    let mut response = db
        .query("SELECT * FROM account")
        .await
        .expect("the select runs");
    let read: Vec<Value> = response.take(0).expect("the account is selected");
    let read: Vec<Account> = read
        .into_iter()
        .map(|record| {
            Account::from_value(record.clone())
                .unwrap_or_else(|error| panic!("the account reads back: {error}\n{record:?}"))
        })
        .collect();
    assert_eq!(read, vec![stored_account]);

    let original = ticket();
    db.query("CREATE ticket:one CONTENT $record")
        .bind(("record", original.clone().into_value()))
        .await
        .expect("the create runs")
        .check()
        .unwrap_or_else(|error| panic!("the ticket matches the schema: {error}\n{schema}"));
    let mut response = db
        .query("SELECT * FROM ticket")
        .await
        .expect("the select runs");
    let read: Vec<Value> = response.take(0).expect("the ticket is selected");
    let read: Vec<Ticket> = read
        .into_iter()
        .map(|record| {
            Ticket::from_value(record.clone())
                .unwrap_or_else(|error| panic!("the ticket reads back: {error}\n{record:?}"))
        })
        .collect();
    assert_eq!(read, vec![original]);
}
