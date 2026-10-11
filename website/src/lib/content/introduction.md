## Your types are the source of truth

Evenframe reads the Rust structs and enums you already write and uses that description for TypeScript generation, SurrealDB schema synchronization, mock data, and runtime metadata. Fields, validators, naming rules, and serialization shapes stay together in Rust.

SurrealDB is the supported database provider. SQL providers are not available in this release.

## Choose your workflow

| Workflow | Use it for | Requirements |
| --- | --- | --- |
| Derive | Validation and validated deserialization | `evenframe` without default features |
| Runtime metadata | Inspecting registered type configurations | `metadata` |
| Build script | Generating types or offline schema files during builds | `build-typesync` plus selected generators, or `build-schemadump` |
| CLI | Type generation, live schema synchronization, mock data, plugins | The `evenframe` binary |

These workflows combine. A project can use the derive at runtime, a build script for TypeScript, and the CLI to synchronize its database.

## How it works

1. **Describe:** derive `Evenframe`, `Typesync`, or `Schemasync` on Rust types.
2. **Scan:** Evenframe finds the source definitions and resolves their type paths.
3. **Generate:** the configured generators produce schemas and TypeScript definitions.
4. **Synchronize:** the CLI compares the schema with SurrealDB and applies changes.

The derive does not emit TypeScript or schema files. Those pipelines work from the workspace scan. A struct with an `id` field is a table; a struct without one is an embedded object.

## A shared validation model

Validators run when Rust reads input, in generated TypeScript, and in the schema's assertions where the stored shape supports them. Morphs normalize a value before validation. Mock data is generated to satisfy its constraints.

Start with **Installation**, then follow **Your first project** for a types-only setup that needs no database.
