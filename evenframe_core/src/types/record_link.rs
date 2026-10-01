//! A link from one record to another: the linked record's id, or the record
//! itself where a query fetched it.

use crate::traits::EvenframeTable;
use crate::types::record_id::parse_record_id;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned, de::Error};
use serde_json::Value;
use surrealdb_types::{Kind, RecordId, SurrealValue};

/// The id half writes as the SDK's `RecordId` does, `{ table, key }` with the
/// key tagged by kind, and the record half as the record does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RecordLink<T: EvenframeTable> {
    Id(RecordId),
    Object(T),
}

impl<'de, T> Deserialize<'de> for RecordLink<T>
where
    T: EvenframeTable + DeserializeOwned,
{
    /// Reads the id as the SDK writes it or as `table:key` text, and an object
    /// as the linked record. A fetched record with fields `T` does not have
    /// falls back to its `id`.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let link_type = std::any::type_name::<T>();
        let value = Value::deserialize(deserializer)?;
        match &value {
            Value::String(text) => parse_record_id(text)
                .map(RecordLink::Id)
                .map_err(|error| D::Error::custom(format!("RecordLink<{link_type}>: {error}"))),
            Value::Object(fields) => {
                let record_error = match T::deserialize(&value) {
                    Ok(record) => return Ok(RecordLink::Object(record)),
                    Err(error) => error,
                };
                let id_error = match RecordId::deserialize(&value) {
                    Ok(id) => return Ok(RecordLink::Id(id)),
                    Err(error) => error,
                };
                let fetched_id = match fields.get("id") {
                    Some(Value::String(text)) => {
                        parse_record_id(text).map_err(|error| error.to_string())
                    }
                    Some(id @ Value::Object(_)) => {
                        RecordId::deserialize(id).map_err(|error| error.to_string())
                    }
                    _ => {
                        return Err(D::Error::custom(format!(
                            "RecordLink<{link_type}>: the object is neither the record \
                             ({record_error}) nor a record id ({id_error})"
                        )));
                    }
                };
                fetched_id.map(RecordLink::Id).map_err(|error| {
                    D::Error::custom(format!(
                        "RecordLink<{link_type}>: the object is not the record \
                         ({record_error}), and its id is not a record id: {error}"
                    ))
                })
            }
            other => Err(D::Error::custom(format!(
                "RecordLink<{link_type}> must be a record id or the record, got {other}"
            ))),
        }
    }
}

impl<T> SurrealValue for RecordLink<T>
where
    T: EvenframeTable + SurrealValue,
{
    fn kind_of() -> Kind {
        Kind::Either(vec![Kind::Record(Vec::new()), T::kind_of()])
    }

    fn is_value(value: &surrealdb_types::Value) -> bool {
        matches!(value, surrealdb_types::Value::RecordId(_)) || T::is_value(value)
    }

    fn into_value(self) -> surrealdb_types::Value {
        match self {
            RecordLink::Id(id) => surrealdb_types::Value::RecordId(id),
            RecordLink::Object(record) => record.into_value(),
        }
    }

    fn from_value(value: surrealdb_types::Value) -> Result<Self, surrealdb_types::Error> {
        match value {
            surrealdb_types::Value::RecordId(id) => Ok(RecordLink::Id(id)),
            record => T::from_value(record).map(RecordLink::Object),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RecordLink;
    use crate::traits::EvenframeTable;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use surrealdb_types::{RecordId, SurrealValue, Value};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SurrealValue)]
    #[serde(deny_unknown_fields)]
    #[surreal(crate = "surrealdb_types")]
    struct Author {
        id: RecordId,
        name: String,
    }

    impl EvenframeTable for Author {}

    fn read(value: serde_json::Value) -> Result<RecordLink<Author>, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn the_id_writes_as_the_sdk_does() {
        let link: RecordLink<Author> = RecordLink::Id(RecordId::new("author", 7_i64));
        assert_eq!(
            serde_json::to_value(&link).expect("serializes"),
            json!({ "table": "author", "key": { "Number": 7 } })
        );
        assert_eq!(
            read(json!({ "table": "author", "key": { "String": "ada" } })).expect("reads"),
            RecordLink::Id(RecordId::new("author", "ada"))
        );
    }

    #[test]
    fn text_ids_keep_their_key_type() {
        assert_eq!(
            read(json!("author:7")).expect("reads"),
            RecordLink::Id(RecordId::new("author", 7_i64))
        );
        assert_eq!(
            read(json!("author:`7`")).expect("reads"),
            RecordLink::Id(RecordId::new("author", "7"))
        );
        assert!(read(json!("author")).is_err());
        assert!(read(json!("")).is_err());
        assert!(read(json!(null)).is_err());
    }

    #[test]
    fn a_fetched_record_is_the_record() {
        let fetched = json!({
            "id": { "table": "author", "key": { "String": "ada" } },
            "name": "Ada",
        });
        assert_eq!(
            read(fetched).expect("reads"),
            RecordLink::Object(Author {
                id: RecordId::new("author", "ada"),
                name: "Ada".to_string(),
            })
        );
    }

    #[test]
    fn a_partly_fetched_record_is_its_id() {
        assert_eq!(
            read(json!({ "id": "author:ada", "bio": "…" })).expect("reads"),
            RecordLink::Id(RecordId::new("author", "ada"))
        );
        assert!(read(json!({ "bio": "…" })).is_err());
    }

    #[test]
    fn surreal_values_carry_the_id_or_the_record() {
        let id = RecordId::new("author", "ada");
        let link: RecordLink<Author> = RecordLink::Id(id.clone());
        assert_eq!(link.into_value(), Value::RecordId(id.clone()));
        assert_eq!(
            RecordLink::<Author>::from_value(Value::RecordId(id.clone())).expect("reads"),
            RecordLink::Id(id.clone())
        );
        let author = Author {
            id,
            name: "Ada".to_string(),
        };
        let record = RecordLink::Object(author.clone()).into_value();
        assert_eq!(
            RecordLink::<Author>::from_value(record).expect("reads"),
            RecordLink::Object(author)
        );
    }
}
