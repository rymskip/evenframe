## Map external types once

External crate types are configured under `general.foreign_types`. Supply the mappings required by the outputs you generate, plus database and mock settings when the type is stored.

```toml
[general.foreign_types.DateTime]
rust_type_names = ["DateTime", "chrono::DateTime"]
ignore_generic_params = true
surrealdb = "datetime"
arktype = { type = "'string.date.iso'" }
effect = { type = "Schema.DateTimeUtc", encoded = "string" }
default_value_surql = "time::now()"
mock_strategy = "datetime"
```

Effect mappings require both `type` and `encoded`. TypeScript mappings can declare an import with `from`, `name`, and `type_only`. Imports are type-only unless specified otherwise. Relative import paths are resolved for each generated output file.

`std::time::Duration` is native. It needs no foreign mapping: the schema uses `duration`, while serde's `{ secs, nanos }` shape is used in TypeScript.

## Record links

Enable `surrealdb-types` to use `RecordLink<T>`. A link contains either the linked record's SDK `RecordId` or the fetched record itself.

The default generated link uses your `RecordId` foreign mapping for its id half. Configure that mapping for every TypeScript output you generate:

```toml
[general.foreign_types.RecordId]
rust_type_names = ["RecordId"]
surrealdb = "record"
arktype = { type = "RecordIdCodec.ark", import = { from = "../record-id.ts", name = "RecordIdCodec", type_only = false } }
effect = { type = "RecordIdCodec.schema", encoded = "Schema.Schema.Encoded<typeof RecordIdCodec.schema>", import = { from = "../record-id.ts", name = "RecordIdCodec", type_only = false } }
macroforge = { type = "RecordIdEncoded", import = { from = "../record-id.ts", name = "RecordIdEncoded" } }
```

The codec module in this example belongs to the consuming application and must actually export those symbols.

## Own the whole link definition

A `RecordLink` foreign entry replaces the link in the outputs it maps. Use `{0}` in its type expression for the linked type. Only its TypeScript mappings and related annotations are configurable; its database shape remains Evenframe's responsibility.

## Missing mappings are errors

Each selected generator checks that foreign types have the mapping it needs. An unknown type should be resolved or explicitly mapped, rather than replaced with a placeholder type.
