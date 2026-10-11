## Select a pipeline

| Derive | Participation |
| --- | --- |
| `Evenframe` | Type generation and schema synchronization |
| `Typesync` | Type generation only |
| `Schemasync` | Schema synchronization only |
| `EvenframeUnion` | An enum whose variants each hold one persistable table type |
| `SurrealValue` | SDK value conversion for query rows that no output describes |

The derive supports structs and enums. Source scanning finds the derive or an attribute named by `general.apply_aliases` that includes it.

## Validation without metadata

Every derived type implements `evenframe::validator::validate::Validate`. Validated fields and morphs also get serde-based deserialization that normalizes and checks input. The table marker is emitted for table types even without metadata.

The scan does not consume the derive's generated configuration functions. A project using the CLI or a build script can therefore disable `metadata` on its runtime dependency.

## Runtime registration

With `metadata`, each derived type registers its fields, shapes, validators, and table settings at compile time. The registry exposes name-based queries, while traits expose a type's static configuration.

```rust
use evenframe::registry::get_table_config;

fn field_names(type_name: &str) -> Option<Vec<String>> {
    get_table_config(type_name).map(|table| {
        table.struct_config.fields.iter()
            .map(|field| field.field_name.clone())
            .collect()
    })
}
```

The registry is keyed by Rust type name: `get_table_config("User")` finds the `User` table, and `get_all_table_names()` lists those type names. Table types provide `EvenframePersistableStruct::static_table_config()`. Metadata is useful for utilities that need type information at runtime, independent of file generation.

## Query row conversion

With `surrealdb-types`, `Evenframe` and `Schemasync` emit SDK value conversion in the stored shape. A standalone `#[derive(evenframe::SurrealValue)]` can describe a query projection. It recognizes serde and surreal naming, defaults, and representation without adding the row to generation pipelines.
