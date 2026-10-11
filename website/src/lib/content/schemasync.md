## Connect to SurrealDB

```toml
[schemasync]
should_generate_mocks = false

[schemasync.database]
url = "${SURREALDB_URL:-http://localhost:8000}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"
```

Evenframe connects over HTTP and signs in as a root user. It reads the credentials from the `SURREALDB_USER` and `SURREALDB_PASSWORD` environment variables, which a command that connects requires. Keep them in your environment or `.env`. The CLI's connection arguments override configuration explicitly. A variable in `.env` is substituted only where configuration references it; it does not become a connection argument on its own.

## Inspect before applying

```sh
evenframe schemasync diff
evenframe schemasync apply
```

`diff` compares the scanned local schema with the database without applying changes. Its output can be pretty, plain, or JSON. `apply --dry-run` shows the intended application; `apply --yes` skips confirmation.

`evenframe schemasync` runs synchronization and then configured mock generation. Use `--no-mocks` when only the schema should change.

## Change detection

Schemasync compares tables, fields, and related definitions, applying changed schema in transactions per table. Mock counts and preservation rules determine whether existing records are kept, trimmed, or regenerated. Record links remain part of the generation plan.

## Offline SurrealQL

```sh
evenframe schemasync dump
evenframe schemasync dump tables
```

The full dump writes `.evenframe/surql/schema.surql`, in apply order: accesses, analyzers, tables, and functions. The tables-only dump writes `.evenframe/surql/tables.surql`. Both accept `-o` for a custom path and require no database connection.

Build scripts can produce the same full schema through `build::schemadump()`.

## Additional database definitions

`schemasync.database.accesses` accepts inline access definitions or a `.surql` path. `functions` and `analyzers` accept paths to SurrealQL files or directories. Analyzers are applied before tables so full-text indexes can refer to them.

## Supported provider

SurrealDB is the only available provider. Configuring an unsupported provider is rejected rather than silently selecting another database.
