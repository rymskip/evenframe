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
    #[validators(email)]
    pub email: String,
    #[morphs(trim)]
    #[validators(non_empty, max_length = 80)]
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

Each validator is named in snake case: a check such as `email` alone, one
with a value as `max_length = 80` or `between = (0.0, 100.0)`. A name two kinds
of validator share takes the kind, as in `string_validator(min_length = 3)`,
and the path form, `StringValidator::MaxLength(80)`, reads the same.

`#[morphs(...)]` rewrites a value into canonical form before its validators
check it, keeping its type:

| Morph | Applies to | Does |
|---|---|---|
| `trim` | `String` | removes leading and trailing whitespace |
| `collapse_whitespace` | `String` | turns each run of whitespace into one space |
| `lower`, `upper` | `String` | changes case |
| `capitalize` | `String` | upper-cases the first character |
| `normalize`, `normalize_nfc`, `normalize_nfd`, `normalize_nfkc`, `normalize_nfkd` | `String` | Unicode normalization (`normalize` is NFC) |
| `round = 2` | `f64` | rounds to that many decimal places, halves up, as `Math.round` does |
| `clamp = ("0", "100")` | integers, `f64`, decimal text in a `String` | pulls the value into the range |
| `sort` | `Vec` of strings, characters, integers or booleans | orders the elements, strings by code point |
| `unique` | `Vec` of strings, characters, integers or booleans | drops elements equal to an earlier one |

Morphs run in the order written, all before the validators, in the Rust read
and in every TypeScript output (Macroforge's `@endec({ normalize: [...] })`).
The schema stores the rewritten value and asserts only the validators. A morph
on a type it does not apply to is a compile error, and one written inside
`#[validators(...)]` is refused with the place it belongs.

A value written as text is a type, so it reads and writes the same way:

| Type | Written as | Holds |
|---|---|---|
| `FromText<T>` | `"42"`, `"0.5"`, `"https://…"` | an integer within JavaScript's safe range, an `f32`/`f64`, or a `url::Url` |
| `IsoDate` | `"2024-02-29T10:15:00.000Z"` | a `DateTime<Utc>` |
| `EpochMillis` | `"1709201700000"` | a `DateTime<Utc>` |
| `JsonText<T>` | `"[1,2]"` | any `T`, read from and written as JSON |

Each parses exactly what ArkType's and Effect's parses accept (`FromText<u8>`
refuses `"300"` and `"007"`), and serializes back to the same text. Validators
and morphs on the field reach the parsed value, the database stores it as its
own type (`int`, `float`, `datetime`, or `T`'s), and the TypeScript outputs
decode the text: ArkType's `string.integer.parse` and the like, Effect's
`NumberFromString` and `parseJson`, and Macroforge's `@endec({ as: ... })`
codecs (`DisplayFromStr`, `DateFromString`, `JsonString`, ...).

A struct whose fields carry `#[validators(...)]` or `#[morphs(...)]` also gets
a generated `serde::Deserialize`: serde reads the input under the struct's own
`#[serde(...)]` attributes, then every field's morphs and validators run, and
every field that fails is reported in one error. Do not derive `Deserialize`
on such a struct yourself. Every derived type also implements
`evenframe::validator::validate::Validate`, whose `validate()` checks a value
built in code, and every value nested in it, the same way. Neither needs
`metadata`: without it the derive emits only these and the `EvenframeTable`
marker that `RecordLink` accepts, which is all a project using only a build
script or the CLI needs.

A single-field tuple struct, or a `#[serde(transparent)]` struct, is a
newtype: serde writes it as its one field's value, and so does every output.
`#[validators(...)]` on the struct checks that value wherever the newtype is
read, and a validator on a field holding a newtype checks the value inside it:

```rust
#[derive(Debug, Clone, Serialize, Evenframe)]
#[validators(non_empty)]
pub struct NonEmptyString(String);

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Team {
    pub id: String,
    #[validators(max_length = 40)]
    pub name: NonEmptyString,
    pub tags: Vec<NonEmptyString>,
}
```

A newtype with validators is also built from a value the program holds:
`NonEmptyString::try_from(text)` runs its validators, rewriting the value as
reading it does, and fails with `ValidationErrors`. One holding a `String`
takes a `&str` too and reads back with `as_str()`. A newtype whose value is a
type parameter gets neither, since core's blanket `TryFrom` already covers it,
and needs a build without the `metadata` feature, whose registry names one
concrete type.

The TypeScript outputs brand a newtype (ArkType and Effect brands,
Macroforge's `$Newtype<T>`), so a plain string is not one. The schema stores
it as its inner type and asserts its validators wherever the value holds it:
the field itself, an `Option`, each element of `tags` above, a map's keys and
values, a tuple's items, an embedded struct's fields, a tagged enum's payload
under its tag and an untagged one's where serde would read the value as that
variant. A struct holding itself, and an untagged variant after one whose
shape SurrealQL cannot state (such as `Maybe(Option<T>)`), cannot be followed
or told apart, so validators there are checked when read but not asserted,
and schemasync warns about each one. The fallback `DEFAULT` of a field whose
zero value fails a check is left out, so the field is required. A
tuple struct of several fields is written as an array and a unit struct as
null, as serde writes them.

A validator runs in three places: the Rust read, the schema's `ASSERT`, and
every TypeScript output. A custom pattern,
`regex_literal = format(custom = "...")`, therefore takes only the syntax
Rust's `regex` and JavaScript's `RegExp` read the same way: no `\d`, `\w`,
`\s`, `\b`, `\p{...}`, `.`, inline flags or class set operations, and a
negated class only directly under `*` or `+`. Anything else is a compile error
naming the portable form, such as `[0-9]` for `\d`.

`starts_with`, `ends_with` and `includes` take text in quotes or a format,
so a password reads `includes = format(uppercase)` rather than a hand-written
pattern. The validator anchors the format's pattern where it looks: a custom
pattern there may not anchor itself with `^` or `$`. Besides the mock-data
formats, `uppercase`, `lowercase`, `digit` and `symbol` (ASCII punctuation)
match one character, and `relative_path` and `slug` match a URL path such as
`/a/b` and a slug such as `my-post-1`.

```rust
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Signup {
    pub id: String,
    #[validators(
        min_length = 8,
        includes = format(uppercase),
        includes = format(digit),
        includes = format(symbol)
    )]
    pub password: String,
}
```

Where one pipeline needs something the others cannot read, an override
replaces a field's, tuple element's or newtype's `#[validators(...)]` there:

```rust
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Member {
    pub id: String,
    #[validators(non_empty)]
    #[typesync(validators(regex_literal = format(custom = "/^\\p{Lu}/u")))]
    #[schemasync(validators(min_length = 2))]
    pub name: String,
}
```

`#[typesync(validators(...))]` applies to the TypeScript outputs, and its
patterns are JavaScript regex literals with their flags (`i`, `m`, `s`, and `u`
or `v`). `#[schemasync(validators(...))]` applies to the schema and mock data,
and its patterns are Rust regexes, which SurrealDB's `string::matches` runs.
Either replaces the list outright for its pipeline. The Rust read runs only
`#[validators(...)]`, so a value with overrides alone is not validated in Rust.
Mock data meets the schema's list and, where it differs, the TypeScript one.

Every other shape serde writes is described as serde writes it:

- A variant's own `#[serde(untagged)]` writes it bare, after the tagged
  variants, which are read first.
- `#[serde(rename(serialize = "...", deserialize = "..."))]` is described by
  the name serde writes, and read by both.
- A key serde skips in one direction only is optional in the TypeScript, and
  the database holds only what serde writes.
- `#[serde(flatten)]` on a struct, or an `Option` of one, puts its fields
  beside the struct's own. A flattened map or enum adds keys known only from
  a value: the TypeScript gains an index signature or an intersection, a table
  holding them is defined SCHEMALESS, and an object holding them is a FLEXIBLE
  `object` field. `[schemasync] warn_schemaless = true` warns about each such
  table.
- `#[serde(into = "X")]` is described as `X` and stored as the struct's own
  fields. With `from` or `try_from`, the read converts first, then the fields'
  validators check the result.
- With `#[serde(remote = "...")]`, the definition's validators check the
  remote value it hands serde.
- Validators on a tuple variant's or tuple struct's elements check each
  element at its position, such as `Circle.0`.

With the `surrealdb-types` feature, every derived type also implements the
SurrealDB SDK's `SurrealValue` in the shape evenframe's schema defines: fields
under their database names (`#[surreal(rename...)]`), enums in serde's
representation, a missing field taking serde's default, and a read that runs
the field's morphs before validating, as serde's read does. Rows read with `response.take::<Vec<T>>(n)` and bound with
`into_value()` need no JSON round trip. A field whose type has no
`SurrealValue` converts through serde. A Rust `Option` is stored as
`option<T>` with NONE by default. Set `[schemasync] option_none = "null"` to
generate `null | T`, NULL defaults, and NULL mock values. For typed SDK writes,
`config.option_none.into_value(record)` applies that configuration without
changing the SDK's canonical `into_value()`. Use
`config.option_none.read_value::<Record>(value)` to read configured NULL
absence through the SDK's declared type shape, including nested optional arrays.

A query's row, which no output describes, takes the same impl from
`#[derive(evenframe::SurrealValue)]`. Its keys are its serde names unless
`#[surreal]` names them, as serde read it from the database's JSON: a
`String` or `Vec<String>` field reads a record id or datetime as that JSON's
text, and a `#[serde(flatten)]` field reads the keys its siblings leave.

Untagged object variants are supported when required keys distinguish their
serialized representations. Optional and defaulted fields are not discriminators.
Field aliases and `deny_unknown_fields` are accounted for; indistinguishable
shapes are rejected at compile time.

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

Macroforge output gives a struct or enum `Decode` by default and a newtype no
derive. A macroforge output's `default_derives = ["Default", "Encode", "Decode"]`
replaces that default for every type with no derives of its own, whether from
the source or an output rule plugin. `#[typesync(...)]` adds to what the outputs
write for a type, struct variant or field:

```rust
#[derive(Debug, Clone, Serialize, Evenframe)]
#[typesync(macroforge(derives = [Default, Encode, Decode]))]
pub struct Profile {
    #[typesync(annotation("@input({ label: \"Name\" })"))]
    pub name: String,
    #[typesync(macroforge(attributes = [endec(rename = "joined_at"), hidden]))]
    pub joined: String,
}
```

`macroforge(derives = [...])` adds to the type's `@derive(...)` and applies to
a struct, enum, newtype or struct variant. `macroforge(attributes = [...])` writes each
entry as the JSDoc annotation Macroforge reads: `hidden` as `/** @hidden */`,
`endec(rename = "joined_at")` as `/** @endec({ rename: "joined_at" }) */`, with
each key in camelCase, a bare key `true` and a nested list an object. Values
are string, number and boolean literals or arrays of them. `annotation("...")`
writes its text as `/** ... */` unchanged. Validation, enum tagging, and
foreign-type format annotations use `@endec`; a foreign type's `endec_format`
option supplies its format annotation. Rust field names and enum representations
still follow their Rust `serde` attributes.

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

A field's `#[format(...)]` shapes its mock values: a `Format` such as
`#[format(Email)]` or `#[format(Url("example.com"))]` generates values its
pattern matches, and a duration field takes a range in steps,
`#[format(duration_ns(min = "PT1H", max = "PT5H", step = "PT15M"))]`. The
bounds are ISO 8601 durations of a fixed length, so years and months are
refused, and the step must divide the range.

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

TypeScript fields default to camelCase. Explicit naming takes precedence:
field `#[evenframe(ts_name = "snake_case")]`, then container
`#[evenframe(all_ts_names = "snake_case")]`, then serde's `rename`, `rename_all`
or `rename_all_fields`. Evenframe casing rules convert the Rust field name.
Supported styles are `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`,
`snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case` and `SCREAMING-KEBAB-CASE`.
An `all_ts_names` on an enum or a named variant applies to its payload fields,
never its variant names.

Set `ts_names = "respect_serde"` under `[typesync]` to use serde's exact names
for fields without an Evenframe override, including unchanged Rust names.
The other accepted value is `"default"`, which is selected when omitted.
Enum variant names and tag/content keys always follow serde. Serde `skip`
omits fields and variants, and `skip_serializing_if` makes a key optional.
TypeScript naming does not change Rust serialization or database naming.

## Database storage

The database stores what serde writes unless a `#[surreal(...)]` key says
otherwise. Every key the SurrealDB SDK's `SurrealValue` derive defines is
accepted where it applies, and the schema, mock data and the derived
`SurrealValue` all follow it:

- On a field: `rename`, `default`, `wrap` (stored as serde writes it, typed
  `any`), `flatten`, and evenframe's `skip`, which leaves a field serde writes
  out of the database.
- On a container: `rename`, `rename_all`, `default`; `tuple` on a tuple struct
  stores it as an array; `value` on a unit struct stores that literal.
- On an enum: `untagged`, `tag`, `content`, `rename_all` (or the legacy
  `uppercase` and `lowercase`), `skip_content` and `skip_content_if`.
- On a variant: `rename`, `rename_all`, `default`, `tuple`, `value` on an
  untagged unit variant, `other` for the unit variant that reads any tag no
  variant names, and `skip_content` or `skip_content_if` on an adjacently
  tagged one.

A key outside these, or one used where it does not apply, is a compile error,
as are the combinations the SDK refuses.

`rename_all` names keys as the SDK does, with one difference: the SDK cases
names with heck 0.4, which splits words at every non-ASCII character and drops
it, while evenframe keeps letters of any script. Under every casing but
`lowercase` and `UPPERCASE`, a field `größe` is stored as `größe` where the
SDK writes `gr_e`, and a variant `ÜberCool` as `über_cool` where it writes
`ber_cool`. ASCII names are stored identically
(`evenframe_derive/tests/surreal_casing.rs`).

A field serde skips is not stored, and a variant serde skips is stored as
NONE, unless the field or variant has a `#[surreal(...)]` attribute or
`#[schemasync(retain)]`, which on a container keeps every field and variant
serde skips. serde writes a unit struct and an untagged unit variant as null,
which is how they are stored. A field with `#[serde(with)]`,
`serialize_with` or `deserialize_with` is stored as that module writes it and
typed `any`. An internally tagged newtype variant stores its tag beside the
keys of the struct it holds; a payload that is not a struct of named fields is
a compile error, where serde would fail when writing it.

## Foreign types

A type from another crate, such as `chrono::DateTime` or
`rust_decimal::Decimal`, is described once under `[general.foreign_types]`:
its database type, its type in each output with the import it needs, its
schema default, and how mock data generates it.

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

`std::time::Duration` needs no entry: it is a SurrealDB `duration` in the
schema and serde's `{ secs, nanos }` in every TypeScript output.

### Record links

A `RecordLink<T>` field holds the linked record's id, the SurrealDB SDK's
`RecordId`, or the record itself where a query fetched it. It needs the
`surrealdb-types` feature. Each TypeScript output writes the id half as the
project's `RecordId` foreign type says, so a project with record links maps
it, for example to a codec for the JavaScript SDK's `RecordId` (the
testground's `tooling/testground/src/record-id.ts` is one):

```toml
[general.foreign_types.RecordId]
rust_type_names = ["RecordId"]
surrealdb = "record"
arktype = { type = "RecordIdCodec.ark", import = { from = "../record-id.ts", name = "RecordIdCodec", type_only = false } }
effect = { type = "RecordIdCodec.schema", encoded = "Schema.Schema.Encoded<typeof RecordIdCodec.schema>", import = { from = "../record-id.ts", name = "RecordIdCodec", type_only = false } }
macroforge = { type = "RecordIdEncoded", import = { from = "../record-id.ts", name = "RecordIdEncoded" } }
```

A `RecordLink` entry instead replaces the whole link in the outputs it maps,
with `{0}` for the linked type.

## Cargo features

`evenframe` with no features builds only what the derive needs.

| Feature | Adds |
| --- | --- |
| `metadata` | Each derived type's config functions and the registry that finds them by name. |
| `surrealdb-types` | `RecordLink`, holding the SurrealDB SDK's `RecordId`, without the SDK's client. |
| `typesync` | Shared type-generation infrastructure, with no output generator selected. |
| `arktype` | The ArkType generator. Enables `typesync`. |
| `effect` | The Effect generator. Enables `typesync`. |
| `macroforge`, `protobuf`, `flatbuffers` | Those generators. `typesync-all` enables every generator. |
| `build-typesync` | Workspace scanning and `build::typesync()` for build scripts. Implies `typesync`. |
| `build-schemadump` | Workspace scanning and `build::schemadump()` for build scripts, with no database client. |
| `build-fullstack` | Both build-script features. |
| `schemasync` | Schema comparison and synchronization against SurrealDB. |
| `mockmake` | Mock data generation. Implies `schemasync`. |
| `wasm-plugins` | The plugin runtime. |
| `cli` (default) | Everything the `evenframe` binary needs. |
| `full` | Every feature. |

## Upgrading to 0.6

- Transforms are morphs: `trim`, `lower`, `upper`, `capitalize` and the
  `normalize` forms move from `#[validators(...)]` to `#[morphs(...)]`, which
  run before every validator rather than in the order written among them.
  The `StringValidator::Trim`-style path forms are gone.
- The parse validators are types: `integer_parse` and `numeric_parse` become a
  `FromText<T>` field, `url_parse` a `FromText<url::Url>`, `date_iso_parse` an
  `IsoDate`, `date_epoch_parse` an `EpochMillis`, and `json_parse` a
  `JsonText<T>`. The loose `date_parse` is gone, since `Date.parse`'s
  formats do not round-trip. A text form's parse error is serde's, reported
  as the read stops, rather than collected with the validators' failures.
- `#[validators(...)]` on a struct of named fields, an enum or a tuple struct
  of several fields is refused: put it on the field, tuple element or newtype
  it checks.
- `starts_with`, `ends_with` and `includes` take a `TextPattern`, text or a
  format, so a path form such as `StringValidator::Includes("x".to_owned())`
  becomes `StringValidator::Includes("x".into())`. An invalid format is
  described by name in error messages rather than by its Rust debug form.
- The ArkType output no longer writes a `default{Type}` object beside each
  struct, and a foreign type's `default_value_ts` is gone: remove it from
  `[general.foreign_types]`, which refuses unknown keys.
- `#[format(AppointmentDurationNs)]` is
  `#[format(duration_ns(min = "PT1H", max = "PT5H", step = "PT15M"))]`, and a
  field's `format` is a `MockFormat`, which wraps a `Format` in
  `MockFormat::Format`.
- `StringValidator::NormalizeNFCPreformatted` and its NFD, NFKC and NFKD twins
  are `NormalizeNfcPreformatted` and so on, and `NumberValidator::NonNaN` is
  `NonNan`, whose attribute name is `non_nan` rather than `non_na_n`.

## License

MIT
