## Brand an inner value

A single-field tuple struct or serde-transparent struct is a newtype. TypeScript outputs brand it, while serde and the database store its inner value.

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[validators(non_empty)]
pub struct NonEmptyString(String);

#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
pub struct Team {
    pub id: String,
    #[validators(max_length = 40)]
    pub name: NonEmptyString,
    pub tags: Vec<NonEmptyString>,
}
```

Container validators check the inner value. Field validators on a field holding a newtype reach that value too. The schema retains newtype validators when it desugars storage to the inner type.

## Validated construction

A validated newtype gets `TryFrom` for its inner value. A string newtype also accepts `&str` and exposes `as_str()`. Morphs run before its validators during construction.

Generic newtypes do not get these constructors because they may overlap with core's blanket `TryFrom`. They also need a build without `metadata`, whose registry describes concrete types.

## Text codecs

Some wire values are text even though their meaning is a parsed value. `evenframe::types` exports a type for each:

| Type | Wire value | Parsed value |
| --- | --- | --- |
| `FromText<T>` | `"42"`, `"0.5"`, or a URL | Integer, float, or `url::Url` |
| `IsoDate` | `"2024-02-29T10:15:00.000Z"` | `DateTime<Utc>` |
| `EpochMillis` | `"1709201700000"` | `DateTime<Utc>` |
| `JsonText<T>` | JSON encoded inside a string | `T` |

These types parse the same forms accepted by the corresponding ArkType and Effect codecs, then serialize back to text. For example, `FromText<u8>` rejects `"300"` and `"007"`. Integer text is restricted to JavaScript's safe range.

Validators and morphs reach the parsed value. The database stores its native form: an integer, float, datetime, or the shape of `T`. TypeScript generators supply the matching decoding operation; Macroforge uses endec codecs.

## Nested validation

Newtype assertions follow options, array elements, map keys and values, tuple positions, embedded fields, and enum payloads. Recursive shapes and untagged variants that cannot be distinguished in SurrealQL are validated on read but cannot always be asserted in storage. Schemasync reports those positions.
