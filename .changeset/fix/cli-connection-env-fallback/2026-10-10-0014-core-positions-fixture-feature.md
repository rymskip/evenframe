# test(core): gate the stored positions fixture on the feature its test needs

- **Date:** 2026-10-10 00:14
- **Commit:** 0339eb8
- **Area:** Core tests

## Problem

A test build without `schemadump` warned that `POSITIONS` is never used.

## Cause

The fixture was declared unconditionally and read only by a `schemadump` test.

## Introduced

dd23fcf (2026-10-07), `feat(core): morphs, text forms and nested schema assertions`. It added the fixture beside its feature-gated test.

## Fix

The fixture carries the same `#[cfg(feature = "schemadump")]`.

## Verified

`cargo test -p evenframe_core --features scan --lib` builds with no warnings: 509 passed.
