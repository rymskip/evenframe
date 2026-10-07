//! Values serde reads and writes as text: a number or URL as its text, an
//! instant as ISO 8601 text or as text of epoch milliseconds, and any value
//! as JSON text. Each parses on deserialize and prints on serialize, so the
//! value round-trips, and each parses exactly what ArkType's and Effect's
//! parses accept. The database stores the parsed value.

use crate::types::field_type::TextFormKind;
use crate::validator::keywords;
use crate::validator::runtime::{Newtype, NewtypeParts};
use crate::validator::validate::{Validate, ValidationErrors};
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod sealed {
    pub trait Sealed {}
}

/// A value [`FromText`] reads from text and writes as text. Sealed: these are
/// the values every output parses alike.
pub trait TextForm: sealed::Sealed + Sized {
    /// What the outputs call this form.
    const KIND: TextFormKind;

    /// The value `text` writes, or what was expected instead.
    fn parse_text(text: &str) -> Result<Self, String>;

    fn print_text(&self) -> String;

    #[cfg(feature = "surrealdb-types")]
    fn stored_kind() -> surrealdb_types::Kind;

    #[cfg(feature = "surrealdb-types")]
    fn into_stored(self) -> surrealdb_types::Value;

    #[cfg(feature = "surrealdb-types")]
    fn from_stored(value: surrealdb_types::Value) -> Result<Self, surrealdb_types::Error>;
}

macro_rules! integer_text_form {
    ($($integer:ty => $kind:ident),*) => {$(
        impl sealed::Sealed for $integer {}

        impl TextForm for $integer {
            const KIND: TextFormKind = TextFormKind::$kind;

            /// ArkType's `string.integer.parse`: a well-formed integer within
            /// the safe range, which must also fit the type.
            fn parse_text(text: &str) -> Result<Self, String> {
                keywords::parse_safe_integer(text)
                    .and_then(|integer| <$integer>::try_from(integer).ok())
                    .ok_or_else(|| {
                        format!(
                            "an integer in the range Number.MIN_SAFE_INTEGER to \
                             Number.MAX_SAFE_INTEGER that fits a {}",
                            stringify!($integer)
                        )
                    })
            }

            fn print_text(&self) -> String {
                self.to_string()
            }

            #[cfg(feature = "surrealdb-types")]
            fn stored_kind() -> surrealdb_types::Kind {
                <$integer as surrealdb_types::SurrealValue>::kind_of()
            }

            #[cfg(feature = "surrealdb-types")]
            fn into_stored(self) -> surrealdb_types::Value {
                surrealdb_types::SurrealValue::into_value(self)
            }

            #[cfg(feature = "surrealdb-types")]
            fn from_stored(value: surrealdb_types::Value) -> Result<Self, surrealdb_types::Error> {
                <$integer as surrealdb_types::SurrealValue>::from_value(value)
            }
        }
    )*};
}

integer_text_form!(
    i8 => I8, i16 => I16, i32 => I32, i64 => I64, isize => Isize,
    u8 => U8, u16 => U16, u32 => U32, u64 => U64, usize => Usize
);

macro_rules! float_text_form {
    ($($float:ty => $kind:ident),*) => {$(
        impl sealed::Sealed for $float {}

        impl TextForm for $float {
            const KIND: TextFormKind = TextFormKind::$kind;

            /// ArkType's `string.numeric.parse`.
            fn parse_text(text: &str) -> Result<Self, String> {
                if keywords::is_numeric(text)
                    && let Ok(number) = text.parse::<$float>()
                {
                    return Ok(number);
                }
                Err("a well-formed numeric string".to_owned())
            }

            fn print_text(&self) -> String {
                self.to_string()
            }

            #[cfg(feature = "surrealdb-types")]
            fn stored_kind() -> surrealdb_types::Kind {
                <$float as surrealdb_types::SurrealValue>::kind_of()
            }

            #[cfg(feature = "surrealdb-types")]
            fn into_stored(self) -> surrealdb_types::Value {
                surrealdb_types::SurrealValue::into_value(self)
            }

            #[cfg(feature = "surrealdb-types")]
            fn from_stored(value: surrealdb_types::Value) -> Result<Self, surrealdb_types::Error> {
                <$float as surrealdb_types::SurrealValue>::from_value(value)
            }
        }
    )*};
}

float_text_form!(f32 => F32, f64 => F64);

impl sealed::Sealed for url::Url {}

/// A URL, stored as its text.
impl TextForm for url::Url {
    const KIND: TextFormKind = TextFormKind::Url;

    fn parse_text(text: &str) -> Result<Self, String> {
        url::Url::parse(text).map_err(|error| format!("a URL string ({error})"))
    }

    fn print_text(&self) -> String {
        self.as_str().to_owned()
    }

    #[cfg(feature = "surrealdb-types")]
    fn stored_kind() -> surrealdb_types::Kind {
        <String as surrealdb_types::SurrealValue>::kind_of()
    }

    #[cfg(feature = "surrealdb-types")]
    fn into_stored(self) -> surrealdb_types::Value {
        surrealdb_types::Value::String(self.into())
    }

    #[cfg(feature = "surrealdb-types")]
    fn from_stored(value: surrealdb_types::Value) -> Result<Self, surrealdb_types::Error> {
        let text = <String as surrealdb_types::SurrealValue>::from_value(value)?;
        Self::parse_text(&text).map_err(|expected| {
            surrealdb_types::Error::serialization(
                format!("a stored URL must be {expected}"),
                surrealdb_types::SerializationError::Deserialization,
            )
        })
    }
}

/// A value written as its text, such as `"42"` for `FromText<i64>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FromText<T: TextForm>(pub T);

/// An instant written as ISO 8601 text, such as `"2024-02-29T10:15:00.000Z"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IsoDate(pub DateTime<Utc>);

/// An instant written as text of milliseconds since the epoch, such as
/// `"1709201700000"`, within the range of a JavaScript `Date`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpochMillis(pub DateTime<Utc>);

/// A value written as JSON text, such as `"[1,2]"` for `JsonText<Vec<u8>>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct JsonText<T>(pub T);

impl IsoDate {
    pub fn parse_text(text: &str) -> Result<Self, String> {
        if keywords::is_iso_8601(text)
            && let Some(instant) = keywords::parse_date(text)
        {
            return Ok(IsoDate(instant));
        }
        Err("an ISO 8601 (YYYY-MM-DDTHH:mm:ss.sssZ) date".to_owned())
    }

    /// The text JavaScript's `Date.prototype.toISOString` writes.
    pub fn print_text(&self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

impl EpochMillis {
    pub fn parse_text(text: &str) -> Result<Self, String> {
        keywords::parse_epoch_millis(text)
            .and_then(|millis| Utc.timestamp_millis_opt(millis).single())
            .map(EpochMillis)
            .ok_or_else(|| "an integer string representing a safe Unix timestamp".to_owned())
    }

    pub fn print_text(&self) -> String {
        self.0.timestamp_millis().to_string()
    }
}

impl<T: DeserializeOwned> JsonText<T> {
    pub fn parse_text(text: &str) -> Result<Self, String> {
        if text.is_empty() {
            return Err("a JSON string, not an empty one".to_owned());
        }
        serde_json::from_str(text)
            .map(JsonText)
            .map_err(|error| format!("a JSON string ({error})"))
    }
}

impl<T: Serialize> JsonText<T> {
    pub fn print_text(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.0)
    }
}

/// Reads text and parses it with `parse`, naming what was expected.
fn read_text<'de, D: Deserializer<'de>, T>(
    deserializer: D,
    parse: impl FnOnce(&str) -> Result<T, String>,
) -> Result<T, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse(&text).map_err(|expected| D::Error::custom(format!("must be {expected}")))
}

impl<T: TextForm> Serialize for FromText<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.print_text())
    }
}

impl<'de, T: TextForm> Deserialize<'de> for FromText<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_text(deserializer, T::parse_text).map(FromText)
    }
}

impl Serialize for IsoDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.print_text())
    }
}

impl<'de> Deserialize<'de> for IsoDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_text(deserializer, IsoDate::parse_text)
    }
}

impl Serialize for EpochMillis {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.print_text())
    }
}

impl<'de> Deserialize<'de> for EpochMillis {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_text(deserializer, EpochMillis::parse_text)
    }
}

impl<T: Serialize> Serialize for JsonText<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let text = self.print_text().map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&text)
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for JsonText<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_text(deserializer, JsonText::parse_text)
    }
}

/// The JSON text's value is checked as itself.
impl<T: Validate> Validate for JsonText<T> {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.0.validate()
    }
}

macro_rules! held_value {
    ($(<$($generic:ident: $bound:path),*> $wrapper:ty => $inner:ty),*) => {$(
        /// A validator or morph on the value checks or rewrites what it holds.
        impl<$($generic: $bound),*> Newtype for $wrapper {
            type Inner = $inner;
            fn inner(&self) -> &Self::Inner {
                &self.0
            }
        }

        impl<$($generic: $bound),*> NewtypeParts for $wrapper {
            fn inner_mut(&mut self) -> &mut Self::Inner {
                &mut self.0
            }
            fn from_inner(inner: Self::Inner) -> Self {
                Self(inner)
            }
        }
    )*};
}

held_value!(
    <T: TextForm> FromText<T> => T,
    <> IsoDate => DateTime<Utc>,
    <> EpochMillis => DateTime<Utc>,
    <T: Sized> JsonText<T> => T
);

#[cfg(feature = "surrealdb-types")]
mod stored {
    use super::{EpochMillis, FromText, IsoDate, JsonText, TextForm};
    use crate::types::field_type::TextFormKind;
    use chrono::{DateTime, Utc};
    use surrealdb_types::{Error, Kind, SurrealValue, Value};

    impl TextFormKind {
        /// `text` read as this form, as the database stores it, for code that
        /// knows a field's type but not its Rust value. The error says what
        /// the text was expected to be.
        pub fn stored_from_text(self, text: &str) -> Result<Value, String> {
            fn stored<T: TextForm>(text: &str) -> Result<Value, String> {
                T::parse_text(text).map(T::into_stored)
            }
            match self {
                TextFormKind::I8 => stored::<i8>(text),
                TextFormKind::I16 => stored::<i16>(text),
                TextFormKind::I32 => stored::<i32>(text),
                TextFormKind::I64 => stored::<i64>(text),
                TextFormKind::Isize => stored::<isize>(text),
                TextFormKind::U8 => stored::<u8>(text),
                TextFormKind::U16 => stored::<u16>(text),
                TextFormKind::U32 => stored::<u32>(text),
                TextFormKind::U64 => stored::<u64>(text),
                TextFormKind::Usize => stored::<usize>(text),
                TextFormKind::F32 => stored::<f32>(text),
                TextFormKind::F64 => stored::<f64>(text),
                TextFormKind::Url => stored::<url::Url>(text),
            }
        }
    }

    /// The database holds the parsed value.
    impl<T: TextForm> SurrealValue for FromText<T> {
        fn kind_of() -> Kind {
            T::stored_kind()
        }

        fn is_value(value: &Value) -> bool {
            T::from_stored(value.clone()).is_ok()
        }

        fn into_value(self) -> Value {
            self.0.into_stored()
        }

        fn from_value(value: Value) -> Result<Self, Error> {
            T::from_stored(value).map(FromText)
        }
    }

    macro_rules! stored_instant {
        ($($wrapper:ident),*) => {$(
            /// The database holds the instant as a datetime.
            impl SurrealValue for $wrapper {
                fn kind_of() -> Kind {
                    <DateTime<Utc>>::kind_of()
                }

                fn is_value(value: &Value) -> bool {
                    <DateTime<Utc>>::is_value(value)
                }

                fn into_value(self) -> Value {
                    self.0.into_value()
                }

                fn from_value(value: Value) -> Result<Self, Error> {
                    <DateTime<Utc>>::from_value(value).map($wrapper)
                }
            }
        )*};
    }

    stored_instant!(IsoDate, EpochMillis);

    /// The database holds the value the JSON text writes.
    impl<T: SurrealValue> SurrealValue for JsonText<T> {
        fn kind_of() -> Kind {
            T::kind_of()
        }

        fn is_value(value: &Value) -> bool {
            T::is_value(value)
        }

        fn into_value(self) -> Value {
            self.0.into_value()
        }

        fn from_value(value: Value) -> Result<Self, Error> {
            T::from_value(value).map(JsonText)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EpochMillis, FromText, IsoDate, JsonText};
    use serde_json::json;

    fn round_trip<T>(text: serde_json::Value) -> T
    where
        T: serde::de::DeserializeOwned + serde::Serialize,
    {
        let value: T = serde_json::from_value(text.clone()).expect("reads");
        assert_eq!(serde_json::to_value(&value).expect("writes"), text);
        value
    }

    #[test]
    fn integers_read_and_write_their_text() {
        assert_eq!(round_trip::<FromText<i64>>(json!("42")), FromText(42));
        assert_eq!(round_trip::<FromText<i64>>(json!("-7")), FromText(-7));
        let read = |text: &str| serde_json::from_value::<FromText<u8>>(json!(text));
        assert!(read("300").is_err());
        assert!(read("007").is_err());
        assert!(read("4.2").is_err());
        assert!(serde_json::from_value::<FromText<i64>>(json!("9007199254740992")).is_err());
        assert!(serde_json::from_value::<FromText<i64>>(json!(42)).is_err());
    }

    #[test]
    fn numbers_and_urls_read_and_write_their_text() {
        assert_eq!(round_trip::<FromText<f64>>(json!("0.5")), FromText(0.5));
        assert!(serde_json::from_value::<FromText<f64>>(json!("1e3")).is_err());
        let url = round_trip::<FromText<url::Url>>(json!("https://example.com/a?b=1"));
        assert_eq!(url.0.host_str(), Some("example.com"));
        assert!(serde_json::from_value::<FromText<url::Url>>(json!("not a url")).is_err());
    }

    #[test]
    fn instants_read_and_write_their_text() {
        let iso = round_trip::<IsoDate>(json!("2024-02-29T10:15:00.000Z"));
        assert_eq!(iso.0.timestamp(), 1_709_201_700);
        assert!(serde_json::from_value::<IsoDate>(json!("yesterday")).is_err());
        let epoch = round_trip::<EpochMillis>(json!("1709201700000"));
        assert_eq!(epoch.0, iso.0);
        assert!(serde_json::from_value::<EpochMillis>(json!("8640000000000001")).is_err());
    }

    #[test]
    fn json_text_reads_and_writes_its_value() {
        assert_eq!(
            round_trip::<JsonText<Vec<u8>>>(json!("[1,2]")),
            JsonText(vec![1, 2])
        );
        assert!(serde_json::from_value::<JsonText<Vec<u8>>>(json!("")).is_err());
        assert!(serde_json::from_value::<JsonText<Vec<u8>>>(json!("[300]")).is_err());
    }

    #[cfg(feature = "surrealdb-types")]
    #[test]
    fn a_kind_reads_text_into_what_the_database_stores() {
        use crate::types::field_type::TextFormKind;
        use surrealdb_types::{Number, Value};
        assert_eq!(
            TextFormKind::I8.stored_from_text("12"),
            Ok(Value::Number(Number::Int(12)))
        );
        assert!(TextFormKind::I8.stored_from_text("300").is_err());
        assert!(TextFormKind::U64.stored_from_text("-1").is_err());
        assert_eq!(
            TextFormKind::Url.stored_from_text("https://example.com"),
            Ok(Value::String("https://example.com/".to_owned()))
        );
        assert!(TextFormKind::F64.stored_from_text("one").is_err());
    }
}
