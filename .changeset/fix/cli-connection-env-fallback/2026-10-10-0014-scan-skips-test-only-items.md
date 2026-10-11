# fix(scan): skip items compiled only for a crate's tests

- **Date:** 2026-10-10 00:14
- **Commit:** ca8292a, 0b83e8e (fixup)
- **Area:** Workspace scanner

## Problem

A stand-in a test module declared with `impl EvenframeTable` became a schema table and a generated TypeScript type in the consuming project.

## Cause

The scanner descended into every inline module, `#[cfg(test)]` ones included, and a manual Evenframe impl opts its type in.

## Introduced

429a3b9 (2026-10-01), `feat(core): use the SurrealDB SDK's types, make Duration native, and split features by use`. It added manual impl detection, so a test's stand-in with a bare `impl EvenframeTable` opts in. Descending into inline modules dates from 52a2239 (2026-10-01).

## Fix

Any struct, enum, impl or module gated on `cfg(test)` is skipped. An out-of-line `#[cfg(test)] mod name;` file is still walked as its own source file.

## Verified

New test `test_scan_rust_file_skips_a_test_module`: red before the fix (it found `User`, `Fixture` and `Probe`), green after. `cargo test -p evenframe_core --features scan --lib`: 509 passed. The fixup formats one call in the scan test, which `verify`'s fmt steps refused; resumed, the run reports all checks passed.
