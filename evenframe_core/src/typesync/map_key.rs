//! Map keys as serde_json writes them: every key becomes a JSON string.

use crate::error::EvenframeError;
use crate::types::{FieldType, ForeignTypeRegistry};

/// What a map key is once serde_json has written it.
pub enum MapKey<'a> {
    /// A `String`, written as itself.
    Text,
    /// A `char`, written as a one-character string.
    Char,
    /// An integer, written as its decimal digits.
    Integer,
    /// A `bool`, written as `"true"` or `"false"`.
    Bool,
    /// A named type: an externally tagged enum of unit variants, or a foreign
    /// type written as a string. [`crate::typesync::checks`] rejects the rest.
    Named(&'a str),
}

impl<'a> MapKey<'a> {
    /// The key's JSON form, or why a map cannot be keyed by it.
    pub fn of(key: &'a FieldType) -> Result<Self, &'static str> {
        match key {
            FieldType::String => Ok(MapKey::Text),
            FieldType::Char => Ok(MapKey::Char),
            FieldType::Bool => Ok(MapKey::Bool),
            FieldType::I8
            | FieldType::I16
            | FieldType::I32
            | FieldType::I64
            | FieldType::I128
            | FieldType::Isize
            | FieldType::U8
            | FieldType::U16
            | FieldType::U32
            | FieldType::U64
            | FieldType::U128
            | FieldType::Usize => Ok(MapKey::Integer),
            FieldType::Other(name) => Ok(MapKey::Named(name)),
            FieldType::F32 | FieldType::F64 => Err(FLOAT),
            FieldType::Unit
            | FieldType::Tuple(_)
            | FieldType::Struct(_)
            | FieldType::Option(_)
            | FieldType::Vec(_)
            | FieldType::HashMap(..)
            | FieldType::BTreeMap(..)
            | FieldType::RecordLink(_)
            | FieldType::Duration
            | FieldType::FromText(_)
            | FieldType::JsonText(_)
            | FieldType::IsoDate
            | FieldType::EpochMillis => Err(UNSUPPORTED),
        }
    }

    /// Whether the key takes a fixed set of values, a `bool` or a scanned
    /// enum, so a map holds any subset of them.
    pub fn is_finite(&self, registry: &ForeignTypeRegistry) -> bool {
        match self {
            MapKey::Bool => true,
            MapKey::Named(name) => registry.lookup(name).is_none(),
            MapKey::Text | MapKey::Char | MapKey::Integer => false,
        }
    }

    /// The key's JSON form, or an error saying why a map cannot be keyed by it.
    pub fn require(key: &'a FieldType) -> crate::error::Result<Self> {
        Self::of(key).map_err(|reason| {
            EvenframeError::type_sync(format!(
                "a map keyed by `{}` {reason}",
                key.canonical_name()
            ))
        })
    }
}

/// Why serde_json cannot write a key, and what to key the map by instead.
pub const UNSUPPORTED: &str = "has no JSON object key: serde_json writes each map key as a \
     string, so key it by a string, a char, an integer, a bool, an externally tagged enum of unit \
     variants or a foreign string type";

/// Why no map is keyed by a float.
pub const FLOAT: &str = "cannot exist: f32 and f64 implement neither Hash nor Ord, so no Rust map \
     can be keyed by them";

/// The values a `bool` key is written as.
pub const BOOL_KEYS: [&str; 2] = ["true", "false"];
