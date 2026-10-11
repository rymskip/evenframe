## Three plugin interfaces

Plugins are WebAssembly modules written with the `evenframe_plugin` crate. Each is a `cdylib` crate built for the `wasm32-unknown-unknown` target.

| Plugin | Configuration | Purpose |
| --- | --- | --- |
| Output rules | `general.output_rule_plugins` | Change annotations, derives, permissions, events, and field rules |
| Synthetic items | `general.synthetic_item_plugins` | Add structs, enums, and tables derived from scanned types |
| Mock data | `schemasync.plugins` | Generate field values for opted-in tables |

The helper crate supplies `define_output_rule_plugin!`, `define_synthetic_item_plugin!`, and `define_mock_data_plugin!` to handle the WASM interface.

## Configure modules

```toml
[general.output_rule_plugins]
conventions = { path = "plugins/conventions.wasm" }

[schemasync.plugins]
people = { path = "plugins/people.wasm", params = { locale = "en" } }
```

Paths are relative to the owning configuration's project root. String parameters support the same environment substitution as other config values.

## Mock plugin context

The mock helper passes a `FieldContext` with the table and field names, field type, record index, total records, record id, and parameters. Return `Some` with the value written as SurrealQL, such as `"'ada@example.com'"`, `"true"`, or `"NONE"`, to handle the field, or `None` so normal generation continues.

A table opts in through `#[mock_data(plugin = "people")]`. Configuring the module alone does not opt every table in.

## Output rules and synthetic items

Output-rule plugins receive the scanned type and pipeline/generator context. They can supply conventions without repeating source attributes. Synthetic-item plugins add definitions derived from scanner results, rather than changing only existing types.

These are separate interfaces: choose one based on whether the plugin changes existing output, creates new types, or generates data.

## Inspect plugin output

```sh
evenframe test-plugin
evenframe test-plugin --type-name Profile
```

The command runs output-rule plugins against the project's types and prints their output as JSON for inspection or scripted assertions. Plugins must be built to their configured `.wasm` paths before a generation run uses them.
