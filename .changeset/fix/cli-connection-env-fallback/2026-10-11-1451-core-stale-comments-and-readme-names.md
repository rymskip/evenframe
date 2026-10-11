# docs(core): correct two stale doc comments and the README's stored key names

- **Date:** 2026-10-11 14:51
- **Commit:** 94a7d29
- **Area:** Core docs, README

## Problem

Three statements disagreed with the code. `CollisionStrategy::AutoRename` said a colliding type is prefixed with its source file name. The constant for Macroforge's default derive carried the description of a function. The README said the database stores what serde writes, which reads as covering key names.

## Cause

Each text outlived the code it described, or said more than the code does. The database stores serde's shape, but a stored key is the Rust name unless `#[surreal(rename)]` or `#[surreal(rename_all)]` changes it.

## Introduced

- The `auto_rename` comment: 429a3b9 (2026-10-01), `feat(core): use the SurrealDB SDK's types, make Duration native, and split features by use`, changed the prefix to the end of the module path under a comment 314a7e2 (2026-03-18) had written for file names.
- The derive comment and the README sentence: cf9559c (2026-10-06), `feat(core): newtypes and every serde wire shape`. It removed `format_derive_line` and put the `DECODE` constant under that function's comment, and it wrote the README's storage section.

## Fix

The `auto_rename` comment names the module path, the constant keeps only its own description, and the README says stored key names are the Rust names unless `#[surreal]` renames them.

## Verified

A schema dump of a struct with `#[serde(rename_all = "camelCase")]` defined its fields under their Rust names, and one with `#[surreal(rename_all = "camelCase")]` under camelCase names. `pixi run verify` passed every step with no warnings.
