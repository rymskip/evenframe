## Cascading configuration

Evenframe reads `.evenframe/config.toml` or `evenframe.toml` from the current directory upward. Files merge with nearer settings winning. Tables merge key by key; other values, including lists, replace the outer value.

Each file resolves its own relative paths and environment variables before merging. A config's relative paths use its own project root. A nearer `.env` wins over an outer one.

## Declare projects

```toml
[general]
projects = ["app/backend", "auth/backend"]

[typesync]
output = { kind = "effect", dir = "./web/src/generated" }
```

Each project keeps its own source, plugin, and database settings while inheriting shared configuration. `projects` is never inherited, and a workspace cannot contain another workspace.

## Share source types

```toml
[general]
include_files = [{ path = "../../app/backend/src/address.rs", resolve_only = true }]
```

`resolve_only` makes a type available for resolution without emitting another copy. The project that owns the type emits it. Two projects that both emit a shared type must define it consistently. Use `exclude_files` to omit sources owned by another project.

## Command scope

From the workspace root, `typesync`, `schemasync`, `generate`, `mockmake`, `validate`, and `check` cover every project, each with its own configuration. Inside a project, `schemasync`, `mockmake`, and `check` cover that project. Typesync still scans every project and writes the shared outputs so common types are generated once.

`info`, `expand`, and `test-plugin` read the nearest configuration as a single project.

## Source scanning

The scanner finds derives in Rust source and resolves type paths. `general.apply_aliases` names attribute macros that include an Evenframe derive.

With `general.expand_macros = true`, the scan also runs `cargo expand` on each crate to find types that macros generate. It requires `cargo-expand` (`cargo install cargo-expand`). Expansions are cached per source file under the Cargo target directory, and `evenframe expand` reports, fills, or clears that cache:

```sh
evenframe expand status
evenframe expand warm
evenframe expand clear
```

## Cache freshness

Scanning commands write `.evenframe/cache.json` in each project's root. `evenframe check` compares its fingerprint with the sources without parsing Rust or connecting to a database. A missing or stale cache exits nonzero, making it useful as a CI gate; from a workspace root it reports every project. Regenerate after source changes before relying on cached mock generation.
