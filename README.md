# Evenframe

Evenframe makes your Rust types the single source of truth. From the structs
and enums you already write, it generates TypeScript types and validators,
synchronizes a SurrealDB schema (SQL planned for a future release), fills the database with mock data, and gives your program the same type information at runtime. You can configure the features to include the full feature set, or something as simple as just using the type metadata with no syncing or mock data at all.

The goal is one dependency for every utility that needs to know your types.
Runtime metadata, type synchronization, schema synchronization and mock data
all start from the same type information, so evenframe reads it once and
serves all of them from one crate, instead of each tool keeping its own copy
of your types.

## Ways to use it

| You want | Use | Needs |
| --- | --- | --- |
| Type metadata and validated deserialization at runtime | the derive | `evenframe` with `metadata` |
| TypeScript and the schema's SurrealQL written on every `cargo build` | the derive and a build script | `evenframe` with `build-typesync`, `build-schemadump` or both (`build-fullstack`) |
| Schema synchronization, mock data and plugins as well | the CLI | the `evenframe` binary |

These combine. A project can use the derive for runtime access, a build script
for its TypeScript, and the CLI for its database.

### 1. The derive: runtime type access

With the `metadata` feature, deriving `Evenframe` on a struct or enum
registers its full description at compile time: fields, types, validators,
and, for tables, permissions, indexes, events and mock data settings. A
struct with an `id` field is a table; one without is an embedded object.
Metadata needs no database dependency.

```toml
[dependencies]
evenframe = { version = "0.6", default-features = false, features = ["metadata"] }
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
`serde::Deserialize`: serde reads the input under the struct's own
`#[serde(...)]` attributes, then every field's validators run, and every field
that fails is reported in one error. Do not derive `Deserialize` on such a
struct yourself. Every derived type also implements
`evenframe::validator::validate::Validate`, whose `validate()` checks a value
built in code, and every value nested in it, the same way. Neither needs
`metadata`: without it the derive emits only these and the `EvenframeTable`
marker that `RecordLink` accepts, which is all a project using only a build
script or the CLI needs.

`Typesync` and `Schemasync` are the same derive limited to one pipeline, and
`EvenframeUnion` describes an enum whose variants are each a table.

### 2. The derive and a build script: generated files on every build

Add evenframe with the `build-typesync` feature as a build dependency, and describe
your outputs in `evenframe.toml` at the project root:

```toml
[build-dependencies]
evenframe = { version = "0.6", default-features = false, features = ["build-typesync"] }
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
    evenframe::build::typesync().expect("type generation failed");
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=evenframe.toml");
}
```

`typesync()` scans the workspace for derived types and writes every
configured output, leaving unchanged files untouched. To configure it in
code instead of the file, build a `ScanConfig` and call
`evenframe::build::typesync_with`.

The outputs are ArkType, Effect, Macroforge, Protocol Buffers and
FlatBuffers. ArkType and Effect are always available; enable `macroforge`,
`protobuf` and `flatbuffers` (or `typesync-all`) for the others. A validator
macroforge does not provide is written as a function in a helpers module beside
the output, which the generated types name in `custom({ function, source })`.

The scan reads source text and finds types by their derive, so the types
still derive `Evenframe` (or `Typesync`, or an `apply_aliases` attribute that
includes it), and the crate depends on `evenframe` for the derive as well. The
scan uses none of the derive's output, so that dependency needs no features.

#### Why a build script does not sync the database

A build script generates files and stops there. Applying schema changes to a
database stays a deliberate step, through the CLI or the `schemasync`
library API, because a build script is the wrong place for it:

- Build scripts run far more often than the builds you mean to make:
  rust-analyzer runs them in the background as you edit, and so do
  `cargo check`, clippy and CI. Each run would change the live database.
- Every build would need the database reachable and its credentials in the
  build environment, so an offline machine or a CI job without the database
  would fail to compile.
- Schemasync is async and needs the full SurrealDB client, which would
  compile into the build dependencies.
- A build script cannot ask before a destructive change, as
  `evenframe schemasync apply` does.

What a build script can do safely is write the schema down. With the
`build-schemadump` feature, `evenframe::build::schemadump()` writes the
SurrealQL schemasync would apply to `.evenframe/surql/schema.surql`, the file
`evenframe schemasync dump` writes, without connecting to a database or
pulling in its client. It needs the `[schemasync]` section of
`evenframe.toml`, whose connection settings may reference variables a build
does not set. `build-fullstack` enables both build-script features:

```rust
// build.rs
fn main() {
    evenframe::build::typesync().expect("type generation failed");
    evenframe::build::schemadump().expect("schema dump failed");
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=evenframe.toml");
}
```

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

## Field names

Each TypeScript output keys a field and tags a variant as serde writes it,
honouring `rename`, `rename_all`, `rename_all_fields` and `skip`, and marks a
key under `skip_serializing_if` optional. The schema and mock data name them as
SurrealValue writes them, honouring `#[surreal(rename)]` and
`#[surreal(rename_all)]`. A field serde or SurrealValue flattens, a field serde
names or skips differently in each direction, and a SurrealValue
representation that differs from serde's are compile errors.

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

`std::time::Duration` needs no entry: it is a SurrealDB `duration` in the
schema and serde's `{ secs, nanos }` in every TypeScript output.

### Record links

A `RecordLink<T>` field holds the linked record's id, the SurrealDB SDK's
`RecordId`, or the record itself where a query fetched it. It needs the
`surrealdb-types` feature. Each TypeScript output writes the id half as the
project's `RecordId` foreign type says, so a project with record links maps
it, for example to a codec for the JavaScript SDK's `RecordId` (the
playground's `src/record-id.ts` is one):

```toml
[general.foreign_types.RecordId]
rust_type_names = ["RecordId"]
surrealdb = "record"
arktype = { type = "RecordIdCodec.ark", import = { from = "../record-id.ts", name = "RecordIdCodec", type_only = false } }
effect = { type = "RecordIdCodec.schema", encoded = "Schema.Schema.Encoded<typeof RecordIdCodec.schema>", import = { from = "../record-id.ts", name = "RecordIdCodec", type_only = false } }
macroforge = { type = "RecordIdEncoded", import = { from = "../record-id.ts", name = "RecordIdEncoded" } }
default_value_ts = "RecordIdCodec.empty"
```

A `RecordLink` entry instead replaces the whole link in the outputs it maps,
with `{0}` for the linked type.

## Cargo features

`evenframe` with no features builds only what the derive needs.

| Feature | Adds |
| --- | --- |
| `metadata` | Each derived type's config functions and the registry that finds them by name. |
| `surrealdb-types` | `RecordLink`, holding the SurrealDB SDK's `RecordId`, without the SDK's client. |
| `typesync` | The ArkType and Effect generators. |
| `macroforge`, `protobuf`, `flatbuffers` | Those generators. `typesync-all` enables every generator. |
| `build-typesync` | Workspace scanning and `build::typesync()` for build scripts. Implies `typesync`. |
| `build-schemadump` | Workspace scanning and `build::schemadump()` for build scripts, with no database client. |
| `build-fullstack` | Both build-script features. |
| `schemasync` | Schema comparison and synchronization against SurrealDB. |
| `mockmake` | Mock data generation. Implies `schemasync`. |
| `wasm-plugins` | The plugin runtime. |
| `cli` (default) | Everything the `evenframe` binary needs. |
| `full` | Every feature. |

## License

MIT
