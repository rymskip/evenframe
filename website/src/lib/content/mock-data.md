## Generate after synchronization

```toml
[schemasync]
should_generate_mocks = true

[schemasync.mock_gen_config]
default_record_count = 100

[schemasync.database]
url = "${SURREALDB_URL}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"
```

`evenframe schemasync` synchronizes the schema before generating configured records. A table's `#[mock_data(n = 50)]` overrides the global count. CLI count arguments can override table settings.

## Shape field values

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[mock_data(n = 50)]
pub struct User {
    pub id: String,
    #[format(Email)]
    #[validators(email)]
    pub email: String,
}
```

`#[format(...)]` selects a mock format, such as `Email` or `Url("example.com")`. Validators constrain the result, and morphs determine its normalized form. Mock data also meets TypeScript overrides when they differ from schema validators.

## Duration ranges

```rust
#[format(duration_ns(min = "PT1H", max = "PT5H", step = "PT15M"))]
```

Duration bounds use fixed-length ISO 8601 durations. Years and months are refused, and the step must divide the range.

## Use the scan cache

```sh
evenframe mockmake --count 500
evenframe mockmake --tables user,team
```

`mockmake` uses `.evenframe/cache.json` without rescanning. The cache must exist and match the sources, the database schema must already be in place, and `should_generate_mocks` must be `true`.

Each selected table is brought to its record count: the records it keeps are regenerated under their ids, missing ones are added, and any beyond the count are deleted. Unselected tables keep their records and can serve as link targets. `--tables` cannot be combined with `full_refresh_mode`.

`evenframe schemasync mock --count 500` is the schema command's mock-only workflow. It scans the sources itself and writes mock data without applying schema changes.

## Preservation and coordination

`mock_gen_config.default_preservation_mode` decides what a changed table's existing records keep: `"Smart"` (the default) rewrites new and modified fields, `"Full"` only new fields, and `"None"` every field. `full_refresh_mode = true` deletes existing records and regenerates every table. Both apply to all tables.

A table's `#[mock_data(...)]` sets its record count, coordination rules, and plugin. Coordination rules under `#[mock_data(coordinate = [...])]` describe relationships between generated values.

For custom generation, configure a named WebAssembly plugin and opt a table in with `#[mock_data(plugin = "people")]`.
