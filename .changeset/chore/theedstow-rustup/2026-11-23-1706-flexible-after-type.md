# fix(schemasync): put FLEXIBLE after TYPE, and on a bare object

- **Date:** 2026-11-23 17:06
- **Commit:** this commit
- **Area:** Schemasync

## Problem

SurrealDB 4 refuses every field schemasync marked flexible: "FLEXIBLE must be specified after
TYPE". A field typed as a bare `object` with no `.*` to type its values, such as a foreign type
mapped to `object`, was never flexible, and SurrealDB 4 refuses every key written into it on a
schemafull table: "Found field 'body.x', but no such field exists".

## Cause

The field statement wrote `FLEXIBLE` before `TYPE`, the order SurrealDB 3 accepted, and only a
field holding a struct with keys known from a value was made flexible. The existing test compared
the generated text, which said `FLEXIBLE TYPE object`, so the order SurrealDB 4 refuses passed.

## Introduced

cf9559c (2026-10-06, "feat(core): newtypes and every serde wire shape") made fields holding open
structs flexible, with `FLEXIBLE` before `TYPE`.

## Fix

`FLEXIBLE` follows the `TYPE`, for computed fields too. A field whose type names `object` itself
outside a string literal, with no `.*` statement typing its values, is flexible; a typed map stays
strict.

## Verified

A new e2e test scans a project with a bare `object` field, an array of objects holding a bare
`object`, a struct with a flattened map and a typed map. It checks the generated statements put
`FLEXIBLE` after `TYPE` on the first three and nowhere on the map, applies the whole schema to an
in-memory SurrealDB 4.0.1, and stores and reads back keys nested inside each. `pixi run verify`
passed every step (run `20261123T214351Z`).
