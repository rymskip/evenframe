## Duplicate Deserialize implementation

If fields have validators or morphs, Evenframe generates a deserializer. Remove a separate `Deserialize` derive from that type. Keep the serde attributes: the generated shadow type uses them.

## A configured generator is unavailable

In a library or build script, enable the generator's feature alongside `build-typesync` or `typesync`. For example, add `arktype` for an ArkType output. The installed CLI includes all generators.

## A custom validator pattern is rejected

Shared validators require portable Rust/JavaScript regex syntax. Replace shorthand classes with explicit ranges, or use `typesync` and `schemasync` validator overrides when the pipelines need different patterns.

## Mockmake reports a stale cache

Run a scanning command after source changes. `evenframe validate --types-only` is the lightest: it rescans without writing outputs or connecting. Confirm that the database schema is in place before running cached mock generation. `evenframe check` reports freshness without a database connection.

## A foreign type is unresolved

Check the source path and imports first. Add an explicit `general.foreign_types` entry for an external crate type, including a mapping for every configured output. Record links require the id representation to be mapped as well.

## A Macroforge derive has no import

Set the output's `macros` entry for a derive that Macroforge does not provide. The package must exist in the consuming project and export that macro.

## Enum payload names collide

Named struct variants produce reusable payload types. If variant names identify different shapes across enums, use `typesync.struct_variants = "inline"`, or rename the source variants deliberately.

## A table becomes SCHEMALESS

A flattened map or enum introduces keys not known from the type alone. This requires an open table. Enable `warn_schemaless` to see which tables have this property, or model fixed keys as a named struct.

## An assertion needs scripting

Some validators lack native SurrealQL equivalents and emit JavaScript assertions. Enable scripting on the SurrealDB server, or configure `schemasync.mock_gen_config.scripting_asserts = false` with awareness that those assertions will not be emitted. Native assertions remain.

## Connection values are missing

Reference environment variables explicitly in `evenframe.toml`. Merely placing a URL in `.env` does not override a literal configured URL. Use `--url`, `--namespace`, or `--database` for deliberate CLI overrides.

## Inspect the actual configuration

```sh
evenframe info --settings
evenframe info --types
evenframe validate
```

Use `-vv` or `RUST_LOG` to inspect scanning and generation diagnostics. Resolve errors at the source definition or configuration, then regenerate outputs.
