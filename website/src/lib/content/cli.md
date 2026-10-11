## Global options

| Option | Meaning |
| --- | --- |
| `--config`, `-c` | Configuration path; also accepts `EVENFRAME_CONFIG` |
| `--output` | Override the directory when generating one output |
| `-v`, `-vv`, `-vvv` | Info, debug, and trace logging |
| `--quiet`, `-q` | Errors only; conflicts with verbose logging |

`RUST_LOG` can set the logging filter. Use `evenframe <command> --help` for the full command-specific options.

## Commands

| Command | Behavior |
| --- | --- |
| `init` | Create configuration; supports `--minimal` and `--force` |
| `typesync` | Generate configured outputs |
| `schemasync` | Synchronize schema, then configured mock data |
| `generate` | Run typesync and schemasync |
| `mockmake` | Insert records from a fresh scan cache |
| `validate` | Validate configuration and detected types |
| `check` | Compare scan cache with sources; stale is nonzero |
| `info` | Display detected types and configuration |
| `test-plugin` | Inspect output-rule plugin results |
| `expand` | Inspect, warm, or clear the macro expansion cache |

Run without a command, `evenframe` does what `generate` does.

## Type generation

```sh
evenframe typesync --formats arktype,effect
evenframe typesync effect --file generated/bindings.ts
```

`--formats` and `--skip` choose among the configured outputs, and `--per-file` writes Effect and Macroforge outputs one file per type. All three go before a subcommand.

Subcommands select `arktype`, `effect`, `macroforge`, `protobuf`, or `flatbuffers`. Each accepts `-o` / `--file`. A kind with no configured output still runs when `-o` or the global `--output` says where to write. Protobuf supports `--package`, `--import-validate`, and `--no-import-validate`; FlatBuffers supports `--namespace`.

## Schema operations

| Subcommand | Important options |
| --- | --- |
| `schemasync diff` | `--format json` or `--format plain` |
| `schemasync apply` | `--dry-run`, `--yes` |
| `schemasync mock` | `--count`, `--tables` |
| `schemasync dump` | `--file`; `tables` subcommand for table DDL only |

`schemasync` itself takes `--url`, `--namespace`, and `--database` overrides, `--no-mocks` to skip data generation, and `--full-refresh` to delete existing records and regenerate them. They go before a subcommand, as in `evenframe schemasync --url http://localhost:8000 diff`.

`apply` asks for confirmation, then runs the same pipeline as `schemasync` alone, mock data included. Offline dumps require no connection.

## Combined generation

`generate` accepts `--skip-typesync`, `--skip-schemasync`, and `--no-mocks`. Choose the narrowest command needed when generating files so unrelated database operations do not run.

## Inspection and validation

`validate` accepts `--config-only`, `--types-only`, and `--check-db`. `info` accepts `--types`, `--settings`, and `--schema`, each a summary read from the configuration and the scan rather than the database, and `--format json` or `--format yaml`. `check --json` emits a machine-readable result: one object for a project, or a list with one object per project from a workspace root.

`expand` requires `status`, `warm`, or `clear`. `mockmake` accepts `--count`, `--tables`, and explicit connection overrides. `test-plugin` accepts `--type-name` to filter by name and `--changed-only false` to include types no plugin changed.
