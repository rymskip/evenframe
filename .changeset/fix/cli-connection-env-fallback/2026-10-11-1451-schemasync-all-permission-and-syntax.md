# fix(schemasync): apply a table's all(...) permission and show the syntax the parser reads

- **Date:** 2026-10-11 14:51
- **Commit:** b60c58a
- **Area:** Schema generation, derive

## Problem

A table with `#[permissions(all("false"))]` was defined `PERMISSIONS FOR select FULL FOR update FULL FOR create FULL FOR delete FULL`: the condition was dropped and every action it should have covered stayed open. A malformed `#[permissions]` also failed with an example, `select = "true"`, that the parser itself refuses.

## Cause

`generate_define_statements` read the four per-action fields of `PermissionsConfig` and fell back to `FULL`, without reading `all_permissions`. The derive's error carried a hand-written example that nothing checked against the parser.

## Introduced

c04827a (2025-08-09), `restructuring`. It added the parser, with its `all(...)` key and parenthesized conditions, beside a DEFINE TABLE that read only the four actions and an error example still written as `key = "value"`.

## Fix

`PermissionsConfig::select`, `create`, `update` and `delete` give an action's own condition, else `all`'s, else `FULL`, and the DEFINE TABLE uses them. The error shows `PermissionsConfig::EXAMPLE`, which a test parses.

## Verified

Four new tests in `schemasync::permissions::tests`; `cargo test -p evenframe_core --features full --lib schemasync::permissions` passed, 4 of 4. A schema dump of a table with `select("true")`, `create("true")`, `update("$auth.id = id")` and `delete("false")` defined exactly those conditions. `pixi run verify` passed every step with no warnings, the 842 core unit tests and the e2e suite against SurrealDB among them.
