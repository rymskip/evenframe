use core::fmt;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, Visitor},
};
#[cfg(feature = "surrealdb")]
use serde::de::MapAccess;
use std::{marker::PhantomData, ops::Deref};

// === EvenframeRecordId: surrealdb-backed implementation ===

#[cfg(feature = "surrealdb")]
use serde::de::MapAccess;
#[cfg(feature = "surrealdb")]
use surrealdb::types::{RecordId, ToSql};

#[cfg(feature = "surrealdb")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvenframeRecordId(pub RecordId);

#[cfg(feature = "surrealdb")]
impl From<String> for EvenframeRecordId {
    fn from(value: String) -> Self {
        let mut parts = value.splitn(2, ':');
        let table = parts.next().unwrap_or("");
        let key = parts.next().unwrap_or("").replace(['⟨', '⟩', '`'], "");
        EvenframeRecordId(RecordId::new(table, key))
    }
}

#[cfg(feature = "surrealdb")]
impl Deref for EvenframeRecordId {
    type Target = RecordId;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "surrealdb")]
impl EvenframeRecordId {
    pub fn as_inner(&self) -> &RecordId {
        &self.0
    }

    pub fn into_inner(self) -> RecordId {
        self.0
    }
}

#[cfg(feature = "surrealdb")]
impl fmt::Display for EvenframeRecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            self.0.to_sql().replace(['⟨', '⟩', '`'], "")
        )
    }
}

#[cfg(feature = "surrealdb")]
impl serde::Serialize for EvenframeRecordId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_sql().replace(['⟨', '⟩', '`'], ""))
    }
}

#[cfg(feature = "surrealdb")]
impl<'de> Deserialize<'de> for EvenframeRecordId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EvenframeRecordIdVisitor;

        impl<'de> Visitor<'de> for EvenframeRecordIdVisitor {
            type Value = EvenframeRecordId;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter
                    .write_str("a RecordId, a string that can be parsed into a RecordId, or null")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let mut parts = value.splitn(2, ':');
                let table = parts.next().unwrap_or("");
                let key = parts.next().unwrap_or("").replace(['⟨', '⟩', '`'], "");
                Ok(EvenframeRecordId(RecordId::new(table, key)))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }

            fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                // fall back to deserializing a full RecordId struct/map
                let record_id = RecordId::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(EvenframeRecordId(record_id))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // JSON `null` → treat as empty string
                self.visit_str("no:access")
            }
        }

        deserializer.deserialize_any(EvenframeRecordIdVisitor)
    }
}

// === EvenframeRecordId: string-based fallback when surrealdb is disabled ===

#[cfg(not(feature = "surrealdb"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvenframeRecordId {
    pub table: String,
    pub key: String,
}

#[cfg(not(feature = "surrealdb"))]
impl From<String> for EvenframeRecordId {
    fn from(value: String) -> Self {
        let mut parts = value.splitn(2, ':');
        let table = parts.next().unwrap_or("").to_string();
        let key = parts
            .next()
            .unwrap_or("")
            .replace(['⟨', '⟩', '`'], "")
            .to_string();
        EvenframeRecordId { table, key }
    }
}

#[cfg(not(feature = "surrealdb"))]
impl fmt::Display for EvenframeRecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.table, self.key)
    }
}

#[cfg(not(feature = "surrealdb"))]
impl serde::Serialize for EvenframeRecordId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}:{}", self.table, self.key))
    }
}

#[cfg(not(feature = "surrealdb"))]
impl<'de> Deserialize<'de> for EvenframeRecordId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct EvenframeRecordIdVisitor;

        impl<'de> Visitor<'de> for EvenframeRecordIdVisitor {
            type Value = EvenframeRecordId;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string in the format 'table:key', or null")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let mut parts = value.splitn(2, ':');
                let table = parts.next().unwrap_or("").to_string();
                let key = parts
                    .next()
                    .unwrap_or("")
                    .replace(['⟨', '⟩', '`'], "")
                    .to_string();
                Ok(EvenframeRecordId { table, key })
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str("no:access")
            }
        }

        deserializer.deserialize_any(EvenframeRecordIdVisitor)
    }
}

// === EvenframePhantomData (always compiled) ===

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvenframePhantomData<T>(pub PhantomData<T>);

impl<T> Deref for EvenframePhantomData<T> {
    type Target = PhantomData<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> EvenframePhantomData<T> {
    pub fn new() -> Self {
        EvenframePhantomData(PhantomData)
    }

    pub fn as_inner(&self) -> &PhantomData<T> {
        &self.0
    }

    pub fn into_inner(self) -> PhantomData<T> {
        self.0
    }
}

// === EvenframeValue (always compiled) ===

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvenframeValue(pub serde_value::Value);

impl Deref for EvenframeValue {
    type Target = serde_value::Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl EvenframeValue {
    pub fn as_inner(&self) -> &serde_value::Value {
        &self.0
    }

    pub fn into_inner(self) -> serde_value::Value {
        self.0
    }
}

// === EvenframeDuration (always compiled) ===

// We remove `Serialize` from the derive macro to provide a custom implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvenframeDuration(pub chrono::TimeDelta);

// Manually implement `Serialize` to control the output format.
impl Serialize for EvenframeDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as a tuple [seconds, nanos]
        use serde::ser::SerializeTuple;

        // Get the total seconds and the nanosecond part
        let total_seconds = self.0.num_seconds();
        let nanos = self.0.subsec_nanos();

        // Create a 2-element tuple
        let mut tuple = serializer.serialize_tuple(2)?;
        tuple.serialize_element(&total_seconds)?;
        tuple.serialize_element(&nanos)?;
        tuple.end()
    }
}

// Deserialize implementation that handles both formats:
// - i64: total nanoseconds (legacy format)
// - [i64, i32]: tuple of [seconds, nanos] (new format)
impl<'de> Deserialize<'de> for EvenframeDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DurationVisitor;

        impl<'de> Visitor<'de> for DurationVisitor {
            type Value = EvenframeDuration;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("either an i64 (nanoseconds) or a tuple [seconds, nanos]")
            }

            // Handle the legacy format: single i64 representing total nanoseconds
            fn visit_i64<E>(self, nanos: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let td = chrono::TimeDelta::nanoseconds(nanos);
                Ok(EvenframeDuration(td))
            }

            // Also handle u64 for large positive values
            fn visit_u64<E>(self, nanos: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let td = chrono::TimeDelta::nanoseconds(nanos as i64);
                Ok(EvenframeDuration(td))
            }

            // Handle i32
            fn visit_i32<E>(self, nanos: i32) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let td = chrono::TimeDelta::nanoseconds(nanos as i64);
                Ok(EvenframeDuration(td))
            }

            // Handle u32
            fn visit_u32<E>(self, nanos: u32) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let td = chrono::TimeDelta::nanoseconds(nanos as i64);
                Ok(EvenframeDuration(td))
            }

            // Handle the new format: tuple of [seconds, nanos]
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let seconds = seq
                    .next_element::<i64>()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let nanos = seq
                    .next_element::<i32>()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;

                // Ensure no extra elements
                if seq.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::invalid_length(3, &self));
                }

                let td = chrono::TimeDelta::seconds(seconds)
                    + chrono::TimeDelta::nanoseconds(nanos as i64);
                Ok(EvenframeDuration(td))
            }
        }

        deserializer.deserialize_any(DurationVisitor)
    }
}
