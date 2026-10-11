# docs(website): correct the documentation against the code

- **Date:** 2026-10-11 14:51
- **Commit:** d8ea9d4
- **Area:** Website, documentation

## Problem

Several pages described behavior the code does not have, and left out things a reader needs.

- Tables: `#[relation]` was a field attribute, and `#[permissions]` and `#[define_field_statement]` had no syntax.
- Naming and storage: database keys were said to follow serde's names.
- Mock data: tables were said to override preservation, and `mockmake`'s need for `should_generate_mocks = true` was missing.
- Build scripts: the scan was said to start at `CARGO_MANIFEST_DIR`.
- Structs and enums: a tuple struct's element error was shown as `Circle.0`.
- CLI and workspaces: `schemasync`'s flags were shown as accepted after a subcommand, and every command was said to cover a workspace.
- Schema synchronization: the `SURREALDB_USER` and `SURREALDB_PASSWORD` a connection requires were not named.

## Cause

The pages were written from the README and the interfaces. Where the README says nothing, such as relations, preservation and command scope, the text was not checked against the code.

## Introduced

bca1d70 (2026-10-09), `feat(website): add SvelteKit documentation on Cloudflare Workers`. It added the pages.

## Fix

Eighteen pages are corrected. `#[relation]` is a struct attribute that makes a relation table, and `#[edge]` is the field attribute. Stored keys are the Rust names unless `#[surreal]` renames them, and `ts_names` governs TypeScript alone. Preservation is one setting for every table. The pages also gain the settings and options they left out: `expand_macros`, `collision_strategy`, `[schemasync.lint]`, Macroforge's built-in derives, the text codecs' module, a plugin's build target and value format, and what `init`, `apply`, `mockmake` and `check` do.

## Verified

- Every Rust example on the pages compiled in a scratch workspace, without features and with `metadata` and `surrealdb-types`, and tests there confirmed the trimmed read, the collected field errors, the newtype constructors, the text codecs, the morph results and the tuple error paths.
- Schema dumps of the table examples showed the relation table, the composite and unique indexes, the permissions, the event, the readonly field, `null | string` under `option_none = "null"`, and stored key names under serde and surreal renames.
- `evenframe validate` and `evenframe typesync` on the quickstart project wrote `web/src/generated/arktype.ts` with the trim and both constraints, and all five generators ran over the page examples.
- `deno task verify` passed: Biome, svelte-check with zero errors and zero warnings, the static build, all five browser tests and Wrangler's dry run.
- A browser walkthrough from the homepage through the navigation opened the tables, naming, configuration and workspaces pages and found the corrected text and examples, with no console errors.
