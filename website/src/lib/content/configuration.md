## Files and environment

Configuration is discovered upward from the current directory and merged from outer files to nearer files. Supported locations are `.evenframe/config.toml` and `evenframe.toml`. `--config` or `EVENFRAME_CONFIG` selects an explicit file.

Tables merge recursively. Other values replace earlier values, including arrays. Relative paths belong to the configuration file's project root.

`${VAR}` reads an environment variable, and `${VAR:-default}` supplies a fallback. A variable's name is uppercase letters, digits, and underscores; any other `${...}` is left as written. `.env` is loaded per config root; `general.env_path` changes its location, relative to the directory of the config file that sets it. A variable already set in the environment is kept, and a nearer file's `.env` wins over an outer file's.

## General settings

| Key | Purpose |
| --- | --- |
| `projects` | Workspace member paths |
| `include_files` | Extra sources, optionally resolve-only |
| `exclude_files` | Sources excluded from the project's scan |
| `apply_aliases` | Attribute macros that include an Evenframe derive |
| `expand_macros` | Run `cargo expand` before scanning, to find types that macros generate |
| `foreign_types` | External type mappings |
| `output_rule_plugins` | WASM conventions for existing types |
| `synthetic_item_plugins` | WASM-derived extra types |
| `env_path` | Environment file location |

## Type generation

```toml
[typesync]
ts_names = "respect_serde"
struct_variants = "inline"
outputs = [
  { kind = "arktype", dir = "./generated/arktype" },
  { kind = "effect", dir = "./generated/effect", mode = "per_file" },
]
```

Use either one `output` or an `outputs` array, never both. Output-specific keys include `file`, `mode`, `barrel_file`, `file_naming`, `file_extension`, `array_style`, and `import_extension`. Macroforge additionally accepts `macros` and `default_derives`; Protobuf accepts `package` and `import_validate`; FlatBuffers accepts `namespace`.

`collision_strategy` decides what happens when two source files define a type of the same name: `"error"` (the default) stops, and `"auto_rename"` prefixes the later type with its module's name.

## Database settings

```toml
[schemasync]
should_generate_mocks = false
warn_schemaless = true

[schemasync.database]
url = "${SURREALDB_URL:-http://localhost:8000}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"
```

`should_generate_mocks` and `database.url` are required. The provider defaults to SurrealDB. The connection timeout defaults to 60 seconds. Accesses, analyzers, and functions can be loaded from SurrealQL paths.

`option_none = "null"` changes absence from NONE to NULL. `mock_gen_config` controls counts, preservation, refresh, and scripting assertions; `plugins` configures mock modules. `[schemasync.lint]` takes `silence_unverifiable_annotations`.

## Narrow configurations

A types-only project needs only `[typesync]`. A schema-only project needs only `[schemasync]`. Build-script schema dumps tolerate unset connection variables because they do not connect.

Use `evenframe info --settings` to see the resolved outputs, mock generation setting, and aliases, and `evenframe validate --config-only` to check the whole file. Unknown keys are rejected.
