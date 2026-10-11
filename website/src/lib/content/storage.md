## The stored representation

The database stores the shape serde writes, overridden by `#[surreal(...)]`: which fields exist, how an enum is represented, what is flattened or skipped. Key names are separate. A stored key is the Rust name unless `#[surreal(rename)]` or `#[surreal(rename_all)]` changes it, whatever serde renames it to. The schema, mock data, and Evenframe's derived SDK conversion share that stored shape.

| Position | Supported examples |
| --- | --- |
| Field | `rename`, `default`, `wrap`, `flatten`, Evenframe's `skip` |
| Container | `rename`, `rename_all`, `default`, tuple storage, unit literal |
| Enum | `untagged`, `tag`, `content`, `rename_all`, content skipping |
| Variant | `rename`, `rename_all`, `default`, `tuple`, `value`, `other`, content skipping |

Unknown keys, keys in the wrong position, and unsupported combinations are compile errors. `wrap` stores the serde representation as `any`.

## Skipped values

A serde-skipped field is not stored. A serde-skipped variant becomes NONE. Add `#[surreal(...)]` or `#[schemasync(retain)]` to retain a skipped position. `retain` on a container retains all its serde-skipped fields and variants.

Unit structs and untagged unit variants store null. Internally tagged newtype variants require a named-field struct payload, so the tag can live beside the payload's keys.

## Optional values

A Rust `Option<T>` is `option<T>` with NONE by default. To use NULL:

```toml
[schemasync]
should_generate_mocks = false
option_none = "null"

[schemasync.database]
url = "${SURREALDB_URL}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"
```

The schema becomes `null | T`, with NULL defaults and mock values. Synchronization rewrites stored absence when this policy changes.

## Typed SDK reads and writes

With `surrealdb-types`, derived values convert through `SurrealValue` in the stored shape. A read runs morphs before validators; a missing field follows serde's default rules. Types without native SDK conversion use serde as the conversion boundary.

For a configured NULL policy, `config.option_none.into_value(record)` writes according to that policy, and `config.option_none.read_value::<Record>(value)` reads configured absence, including nested optional arrays. The canonical SDK `into_value()` remains unchanged.

## Open and recursive shapes

Flattened maps and enums can introduce unknown keys. Tables holding them are SCHEMALESS; embedded fields are FLEXIBLE. Recursive or ambiguous untagged shapes may prevent nested assertions even when Rust read validation is available.
