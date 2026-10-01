# Evenframe

Evenframe makes your Rust types the single source of truth. From the structs
and enums you already write, it generates TypeScript types and validators,
synchronizes a SurrealDB schema, fills the database with mock data, and gives
your program the same type information at runtime.

The goal is one dependency for every utility that needs to know your types.
Runtime metadata, type synchronization, schema synchronization and mock data
all start from the same type information, so evenframe reads it once and
serves all of them from one crate, instead of each tool keeping its own copy
of your types.

## Ways to use it

| You want | Use | Needs |
| --- | --- | --- |
| Type metadata and validated deserialization at runtime | the derive | `evenframe`, no features |
| TypeScript generated on every `cargo build` | the derive and a build script | `evenframe` with `tooling` |
| Schema synchronization, mock data and plugins as well | the CLI | the `evenframe` binary |

These combine. A project can use the derive for runtime access, a build script
for its TypeScript, and the CLI for its database.

### 1. The derive: runtime type access

Deriving `Evenframe` on a struct or enum registers its full description at
compile time: fields, types, validators, and, for tables, permissions,
indexes, events and mock data settings. A struct with an `id` field is a
table; one without is an embedded object.

```toml
[dependencies]
evenframe = { version = "0.6", default-features = false }
serde = { version = "1", features = ["derive"] }
```

```rust
use evenframe::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 50)]
pub struct User {
    pub id: String,
    #[validators(StringValidator::Email)]
    pub email: String,
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(80))]
    pub name: String,
}
```

At runtime, look a type up by name in the registry, or ask it directly:

```rust
use evenframe::registry::{get_all_table_names, get_table_config};
use evenframe::traits::EvenframePersistableStruct;

let user = get_table_config("User").expect("User is registered");
for field in &user.struct_config.fields {
    println!("{}: {}", field.field_name, field.field_type);
}
println!("tables: {:?}", get_all_table_names());
assert_eq!(User::static_table_config().table_name, user.table_name);
```

A struct whose fields carry `#[validators(...)]` also gets a generated
`serde::Deserialize` that runs them, so invalid input fails to deserialize.
Do not derive `Deserialize` on such a struct yourself.

`Typesync` and `Schemasync` are the same derive limited to one pipeline, and
`EvenframeUnion` describes an enum whose variants are each a table.

### 2. The derive and a build script: TypeScript on every build

Add evenframe with the `tooling` feature as a build dependency, and describe
your outputs in `evenframe.toml` at the project root:

```toml
[build-dependencies]
evenframe = { version = "0.6", default-features = false, features = ["tooling"] }
```

```toml
# evenframe.toml
[typesync]
outputs = [
  { kind = "arktype", dir = "./web/src/generated" },
  { kind = "effect", dir = "./web/src/generated/effect", mode = "per_file" },
]
```

```rust
// build.rs
fn main() {
    evenframe::tooling::generate().expect("type generation failed");
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=evenframe.toml");
}
```

`generate()` scans the workspace for derived types and writes every
configured output, leaving unchanged files untouched. To configure it in
code instead of the file, build a `BuildConfig` and call
`evenframe::tooling::generate_with_config`.

The outputs are ArkType, Effect, Macroforge, Protocol Buffers and
FlatBuffers. ArkType and Effect are always available; enable `macroforge`,
`protobuf` and `flatbuffers` (or `typesync-all`) for the others.

### 3. The CLI: schemasync, typesync, or both

The CLI drives the whole pipeline from `evenframe.toml`: generating types,
synchronizing the database schema, writing mock data, and running plugins.

```sh
cargo install evenframe
evenframe init
```

| Command | What it does |
| --- | --- |
| `evenframe typesync` | Writes every configured TypeScript and schema output (`--formats` and `--skip` pick kinds). |
| `evenframe schemasync` | Brings the database schema in line with the Rust types, then writes mock data. |
| `evenframe schemasync diff` | Shows the schema changes without applying them. |
| `evenframe schemasync dump` | Writes the resolved SurrealQL to a file, with no database connection. |
| `evenframe generate` | Runs typesync and schemasync together (`--skip-typesync`, `--skip-schemasync`, `--no-mocks`). |
| `evenframe mockmake --count 500` | Inserts mock records from the scan cache, without rescanning the sources. |
| `evenframe validate` | Checks the configuration and the detected types. |
| `evenframe check` | Exits non-zero when the scan cache no longer matches the sources, for CI. |
| `evenframe info` | Lists the detected types and configuration. |
| `evenframe test-plugin` | Runs an output rule plugin against the project and prints what it produces. |
| `evenframe expand` | Manages the macro expansion cache used for types that macros generate. |

A project that only syncs its database needs only a `[schemasync]` section,
and one that only generates types needs only `[typesync]`:

```toml
[schemasync]
should_generate_mocks = true

[schemasync.database]
provider = "surrealdb"
url = "${SURREALDB_URL:-http://localhost:8000}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"
timeout = 60

[schemasync.mock_gen_config]
default_record_count = 100

[typesync]
outputs = [{ kind = "arktype", dir = "./web/src/generated" }]
```

`evenframe.toml` is found by searching upward from the current directory.
Values can reference environment variables as `${VAR}` or `${VAR:-default}`,
and a `.env` file next to the config is loaded first.

Schemasync compares the types with the live database and applies only what
changed, one transaction per table. Existing records are kept, trimmed or
regenerated to each table's mock count, and links between records stay
valid.

#### Plugins

Plugins are WebAssembly modules, written in Rust with the `evenframe_plugin`
crate:

- **Output rule plugins** (`[general.output_rule_plugins]`) see every type
  and change how it is generated: annotations, derives, permissions, events
  and field rules.
- **Synthetic item plugins** (`[general.synthetic_item_plugins]`) add new
  structs, enums and tables derived from the scanned types.
- **Mock data plugins** (`[schemasync.plugins]`) generate values for a
  table's fields; a table opts in with `#[mock_data(plugin = "name")]`.

```toml
[general.output_rule_plugins]
conventions = { path = "plugins/conventions.wasm" }

[schemasync.plugins]
people = { path = "plugins/people.wasm", params = { locale = "en" } }
```

## Foreign types

A type from another crate, such as `chrono::DateTime` or
`rust_decimal::Decimal`, is described once under `[general.foreign_types]`:
its database type, its type in each output with the import it needs, its
default value, and how mock data generates it.

```toml
[general.foreign_types.DateTime]
rust_type_names = ["DateTime", "chrono::DateTime"]
ignore_generic_params = true
surrealdb = "datetime"
arktype = { type = "'string.date.iso'" }
effect = { type = "Schema.DateTimeUtc", encoded = "string" }
default_value_ts = "new Date().toISOString()"
default_value_surql = "time::now()"
mock_strategy = "datetime"
```

## Cargo features

`evenframe` with no features builds only what the derive needs.

| Feature | Adds |
| --- | --- |
| `typesync` | The ArkType and Effect generators. |
| `macroforge`, `protobuf`, `flatbuffers` | Those generators. `typesync-all` enables every generator. |
| `tooling` | Workspace scanning and `tooling::generate()` for build scripts. Implies `typesync`. |
| `schemasync` | Schema comparison and synchronization against SurrealDB. |
| `mockmake` | Mock data generation. Implies `schemasync`. |
| `wasm-plugins` | The plugin runtime. |
| `cli` (default) | Everything the `evenframe` binary needs. |
| `full` | Every feature. |

## License

MIT
