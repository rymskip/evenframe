# Repository Guidelines

Evenframe is a Rust framework for TypeScript type generation and database schema
synchronization with SurrealDB.

## Project Structure & Module Organization
- `evenframe/`: CLI + library entrypoint (`src/main.rs`, `src/lib.rs`).
- `evenframe_core/`: core logic: config, schemasync, typesync, logging macros.
- `evenframe_derive/`: procedural macros used by the core/CLI crates.
- `tooling/`: the repo's task runner (`cargo run -p tooling -- verify`, `test`, `snapshot`, `generated`, `bump`), plus:
  - `verify` applies the formatter and clippy's machine-applicable fixes before it checks; `--check` leaves the tree as it is and only reports. It records each run's progress in `.verify/<id>.json`; `--pipeline a,b` runs the named steps in order and `--resume [id]` reruns every step a run (the latest by default) did not pass.
  - `tooling/derive_check/`: the derive in a crate depending only on `evenframe`, tested without features, with `metadata` and with `surrealdb-types`.
  - `tooling/tests_macros/`: the `gen_tests!` macro that turns snapshot spec files into tests.
  - `tooling/generated_check/`: the deno project that type-checks generated TypeScript.
  - `tooling/testground/`: an Axum app with its own workspace and test plugins, the subject of the e2e suite (`cargo run -p tooling -- test --e2e`).
- `target/`: build artifacts (ignored). Debug log files go to the directory `EVENFRAME_LOG_DIR` names, and nowhere while it is unset.

`evenframe_core` features: with none, it builds only what `evenframe_derive`
needs (types, config, validators, the schemasync data types), and the derive
emits only `Validate`, validated deserialization and the `EvenframeTable` marker.
`metadata` adds the derive's config functions and the registry. `typesync`
adds the generators. `scan` (workspace scanning and the macro expansion cache)
backs `build-typesync` (`build::typesync()` for build scripts) and
`build-schemadump` (`build::schemadump()`, the schema's SurrealQL without a
database client); `build-fullstack` enables both. `schemadump` is the
offline SurrealQL generation, which `schemasync` extends with the database
engine; `mockmake` and `wasm-plugins` add mock data and plugins;
`surrealdb-types` adds `RecordLink` and the derive's `SurrealValue`. `cli` is the binary's set and `full`
enables everything. The scan cache and the process lock live in the
`evenframe` crate, which only the CLI uses them from.

## Build, Test, and Development Commands
- Build all crates: `cargo build --workspace`
- Release build: `cargo build --release`
- Run the CLI: `RUST_LOG=info cargo run -p evenframe`; set `EVENFRAME_LOG_DIR` to a directory to keep its debug log files
- Tests: `cargo test --workspace --all-features`
- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Notes
- Configuration is found upward from the current directory, in `.evenframe/config.toml` or `evenframe.toml`, and every config up the tree is merged, nearer over outer. Each config root's `.env` is loaded.

## Architecture

### Core Components

1. **schemasync** (`evenframe_core/src/schemasync/`): schema synchronization with SurrealDB
   - `compare/`: schema comparison and change detection
   - `mockmake/`: mock data generation
   - `database/surql/`: SurrealQL generation (`define.rs`, `assert.rs`, `access.rs`) and execution (`execute.rs`); `shape.rs` walks a field's stored type once for both its `TYPE` and an `ASSERT` checking the validators at every position (object members, elements, map keys and values, tuple items, a tagged enum's payload under its tag, an untagged variant by serde's first match), deciding whether the fallback `DEFAULT` meets every check, and warning where it cannot assert (variants after one whose shape SurrealQL cannot state, recursive or open structs)
   - `dump.rs`: the offline schema dump shared by the CLI and build scripts
   - `table.rs`, `edge.rs`: table and edge configurations

2. **typesync** (`evenframe_core/src/typesync/`): generates type definitions
   - `arktype.rs`, `effect.rs`, `macroforge.rs`: ArkType, Effect and Macroforge TypeScript
   - `protobuf.rs`, `flatbuffers.rs`: Protocol Buffers and FlatBuffers schemas

3. **Derive** (`evenframe_derive/`, with attribute parsing in `evenframe_core/src/derive/`)
   - Emits `Validate` for every type, validated deserialization through a serde-derived shadow of a struct or enum with field validators or morphs, the `EvenframeTable` marker for tables, with `metadata` each type's config functions and registry entry, and with `surrealdb-types` a `SurrealValue` in the schema's shape (`surreal_value_impl.rs`, runtime half in `evenframe_core/src/surreal_value.rs`)
   - A Rust `Option` defaults to `option<T>` / NONE in the schema. `[schemasync] option_none = "null"` selects `null | T` / NULL; schemasync rewrites stored absence when that policy changes (`database/surql/optional.rs`)
   - Field and variant names outside Rust come from `derive/naming.rs`, shared with the scanner: TypeScript fields use `ts_name()` with camelCase by default, explicit Evenframe casing before serde naming, or `[typesync] ts_names = "respect_serde"` for serde's fallback; variant tags remain serde-controlled. Database keys use `db_name()`
   - A single-field tuple struct or `#[serde(transparent)]` struct is a newtype (`newtype_impl.rs`, `types/newtype.rs`): container validators check its value, the derive implements `validator::runtime::Newtype` so field validators reach the value inside and, with validators, a `TryFrom` its value through them (`generate_newtype_constructors`), typesync brands it, and `AllConfigs::into_schemasync` stores it as its inner type through `desugar_newtypes`, keeping the declared types in `SchemasyncTypes::declared`, from which mockmake and the schema read the validators of a newtype below a field's own value
   - Every serde shape is described as serde writes it (`derive/naming.rs`): a variant's own `untagged` (`Wire::serde_untagged`, `Variant::serde_representation`), split rename names (deserialize name read as an alias), one-way skips (`FieldHandling`, optional in TypeScript), `flatten` (struct fields expanded by `AllConfigs::for_typesync`/`into_schemasync`, maps and enums leaving the struct open: SCHEMALESS table or FLEXIBLE object), `into` (a typesync-only alias newtype beside a schemasync-only struct), `from`/`try_from` and `remote` reads in `deserialization_impl.rs`, and tuple element validators (`element_validators`). `Wire::storage` says what the database holds, which the schema and `SurrealValue` share
   - Storage follows serde's shape, overridden by each `#[surreal]` key the SDK's derive defines (`derive/surreal_attributes.rs`, read per position, unknown keys refused). A serde-skipped field is not stored and a serde-skipped variant is NONE unless the item has `#[surreal]` or `#[schemasync(retain)]` (also on the container); `#[serde(with)]` fields convert through a private serde shim and are typed `any`. A newtype whose own keys change its stored shape (`NewtypeConfig::storage`) desugars into a one-variant untagged enum carrying that storage
   - `#[morphs(...)]` (`validator/morph.rs`: `StringMorph`, `NumberMorph`, `ArrayMorph`) rewrites a value before its validators check it; morphs live in their own `morphs` field beside `validators` (and `element_morphs` beside `element_validators`), run in Rust through `validator::runtime`'s target traits, become ArkType/Effect transformations (`js_checks::morph_expression`), Macroforge's `@endec({ normalize })` and FlatBuffers' `normalize` attribute, shape mock data, and add no ASSERT. `#[validators]` refuses a morph's name
   - Validators run in Rust, in the schema and in TypeScript, so a custom pattern in `#[validators]` must be in the portable subset (`validator/portable_regex.rs`). `#[typesync(validators(...))]` (JavaScript literals, checked with `regress`) and `#[schemasync(validators(...))]` (Rust regex) replace the list for one pipeline (`ValidatorOverrides`, swapped in by `AllConfigs::for_typesync`/`into_schemasync`); the Rust read runs only `#[validators]`. The schemasync view keeps the TypeScript list in `ValidatorOverrides::typesync` where it differs, which mock data also meets
   - `message = "..."` in `#[validators]` (`FieldValidators::message`, `validator_message` on a field or newtype config) replaces what every validator in the list reports: the derive's check pushes it in place of the rejection, ArkType wraps the definition in `[def, '@', { message }]` (which covers a wrong-type value too), Effect's filter messages and Macroforge's `{ validate, message }` carry it. A value has one message, so the parser refuses a second one, a message with no validators, and a message in a pipeline override; a SurrealDB `ASSERT` carries none
   - Generates no TypeScript or schema: typesync and schemasync work from the scan

### Workflow

1. **Scanning**: `scan/workspace.rs` finds all Rust types with Evenframe derives, resolving type paths through `scan/paths.rs`
2. **Config building**: `scan/configs.rs` processes found types into internal configs
3. **Typesync**: creates TypeScript definitions (ArkType, Effect, Macroforge) and schema files
4. **Schemasync**: compares local schemas with the database and applies changes
5. **Mock generation**: optionally generates test data based on schemas

## Configuration

Configuration (`.evenframe/config.toml` or `evenframe.toml`) is found from the
current directory upward and merged, nearer over outer. Environment variables
from `.env` are referenced with `${VAR_NAME}` (defaults via `${VAR:-default}`).

Key sections:
- `general.apply_aliases`: custom attribute macros that include the Evenframe derive
- `general.foreign_types`: how an external Rust type maps to each output. Each
  TypeScript output (`arktype`, `effect`, `macroforge`) takes
  `{ type, import = { from, name, type_only } }`, with `encoded` on `effect`; `from`
  is a package or a path relative to each generated file. A `RecordLink` entry
  may set only these TypeScript mappings, with `{0}` for the linked type, to
  point generated files at the project's own `RecordLink`; without one,
  evenframe declares `RecordLink` itself as the project's `RecordId` foreign
  type (the SDK's record id, which a project with record links must map) or
  the linked record. `std::time::Duration` is native and needs no entry
- `schemasync`: database connection and mock generation settings;
  `warn_schemaless` warns about each table defined SCHEMALESS because its
  record holds a flattened map's or enum's keys
- `typesync`: the generated outputs, as one `output = { kind, dir, ... }` or an
  `outputs = [ ... ]` array; each kind writes to its own `dir`. A macroforge
  output imports each derive macroforge does not provide from the package its
  `macros = { Derive = "package" }` names, and stops on a derive with none;
  its `default_derives` sets the derives of a type with none of its own (unset:
  `Decode` on a struct or enum, nothing on a newtype)
- `typesync.struct_variants`: `"named"` (default) declares each struct variant's
  fields as a type named after the variant, which the enum references; `"inline"`
  writes them inside the enum, for projects whose variant names clash

```
[schemasync.database]
url = "${DB_URL:-http://localhost:8000}"
namespace = "${DB_NS:-test}"
database = "${DB_NAME:-test}"
timeout = 60

[typesync]
outputs = [
  { kind = "arktype", dir = "./generated/arktype" },
  { kind = "effect", dir = "./generated/effect", mode = "per_file" },
]
```

## Key Dependencies

- `surrealdb`: database client for schema synchronization
- `syn`, `quote`, `proc-macro2`: Rust macro and AST manipulation
- `tokio`: async runtime
- `tracing`: structured logging (`RUST_LOG=debug` for verbose output)

## Coding Style & Naming Conventions
- Rust idioms; 4-space indentation; keep Clippy clean (`-D warnings`).
- Names: `snake_case` for modules/functions, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for consts.
- Prefer `Result` over `panic!`; use `tracing` for logs; avoid `println!`.

## Testing Guidelines
- Place unit tests inline with modules using `#[cfg(test)]` and `#[test]`.
- Name tests descriptively (e.g., `parses_config_with_defaults`).
- Every test build uses one feature resolution, `--workspace --all-features`, the one clippy checks. Narrow a run with a test name or `--test <target>` (`cargo test --workspace --all-features <name>`), never `-p` or `--features`: Cargo unifies a dependency's features only within one invocation, so another selection rebuilds the whole dependency tree.

## Commit & Pull Request Guidelines
- Commits: `type(scope): subject` in the imperative mood. Example: `fix(typesync): name each struct variant's fields after the variant`.
- PRs: include a clear summary, linked issues, a "Verifications" section (commands run), and docs updates/screenshots where applicable. Ensure `fmt` + `clippy` + tests pass.

## Changesets
Every working branch records its changes under `.changeset/<branch>/`, the branch name used as the path (`.changeset/fix/cli-connection-env-fallback/`). Add an entry in the same commit as the change it describes, or right after, so the record never trails the branch.
- One file per change, named `YYYY-MM-DD-HHMM-<slug>.md` from the commit's date.
- Each file opens with the commit subject as its heading, then **Date**, **Commit** and **Area**, then `## Problem`, `## Cause`, `## Introduced`, `## Fix` and `## Verified`. Verified says what actually ran and its result, never what should pass.
- Introduced names the commit that caused the problem, with its date and subject, and says what it changed. Trace the cause, not only the blamed lines: when a line was correct until another change broke it, name that change and the line's origin. A change that fixes nothing, such as a feature, leaves it out.
- A `fixup!` commit is listed on the entry of the change it amends, not as its own entry.
- `index.md` lists every entry in a table of date, commit, linked title and the introducing commit, in commit order.
- A change made in another repository is recorded in that repository's own `.changeset/<branch>/`, never here.

## Security & Configuration Tips
- Do not commit secrets. Use `.env`; reference via `${VAR}` in `evenframe.toml`.
- Database access is configured under `[schemasync.database]`; keep credentials in env.
- Schemasync signs in with `SURREALDB_USER` and `SURREALDB_PASSWORD` from the environment;
  the URL, namespace and database come from configuration.
