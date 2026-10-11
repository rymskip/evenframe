## Follow serde's wire shape

Evenframe describes a value as serde writes it, with database-specific overrides from `#[surreal(...)]`. Serialization attributes therefore affect more than Rust JSON: they shape generated definitions, database storage, and mock records.

## Struct shapes

| Rust shape | Serialized representation |
| --- | --- |
| Named-field struct | Object |
| Single-field tuple struct | Its inner value (newtype) |
| `#[serde(transparent)]` struct | Its inner value (newtype) |
| Multiple-field tuple struct | Array with positional elements |
| Unit struct | Null |

Validators and morphs can be attached to the elements of a tuple struct or tuple variant. Their errors identify the position: `0` for a tuple struct's first element, `Circle.0` for the first element of a `Circle` variant.

## Enum representations

Externally tagged, internally tagged, adjacently tagged, and untagged serde representations are supported. Variant names and tag/content keys remain serde-controlled even when TypeScript payload fields use different casing.

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[serde(tag = "kind", content = "value")]
pub enum Contact {
    Email(String),
    Phone(String),
}
```

A variant's own `#[serde(untagged)]` writes it bare. Tagged variants are read before those bare variants. Untagged database object variants need required keys that distinguish their serialized representations; optional and defaulted fields do not distinguish them. Aliases and `deny_unknown_fields` are considered.

## Skips and renames

Split serde names use the serialize name in outputs and accept the deserialize name as an alias when reading. A one-direction skip makes a key optional in TypeScript. Database storage follows what serde writes.

## Flattening

Flattening a struct or an optional struct expands its fields into the containing object. A flattened map or enum adds keys only a value can determine. Such tables are SCHEMALESS, and embedded objects are FLEXIBLE. Set `schemasync.warn_schemaless = true` to report those tables.

## Conversions and custom serializers

`#[serde(into = "X")]` describes the TypeScript wire value as `X`, while schema storage uses the struct's own fields. `from` and `try_from` convert before validating the resulting fields. With `remote`, validators check the remote value handed to serde.

Fields using `with`, `serialize_with`, or `deserialize_with` store the custom serde representation. Their schema type is `any`, because an arbitrary serializer's shape cannot be inferred from the Rust field alone.
