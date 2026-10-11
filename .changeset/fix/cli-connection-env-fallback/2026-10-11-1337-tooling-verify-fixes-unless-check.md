# feat(tooling): have verify fix what it can unless --check is passed

- **Date:** 2026-10-11 13:37
- **Commit:** 81e5ac5
- **Area:** Tooling, verify

## Problem

`verify` failed on unformatted code and on lints clippy can fix itself, and left both for a second command and a rerun.

## Cause

The fmt steps ran `cargo fmt -- --check` and the clippy steps ran the gate alone. Nothing in a run wrote to the tree.

## Fix

A run applies the formatter, and in each clippy step applies clippy's machine-applicable fixes and formats what they rewrote before the gate runs. `--check` leaves the tree as it is and only reports. Both hold for the workspace and for the testground.

## Verified

Three new tests in `verify::tests`; `cargo test -p tooling` passed, 5 of 5. Against a function left unformatted with a `needless_return`: `verify --check --pipeline fmt,clippy` failed both steps and left the file's hash unchanged; `verify --pipeline fmt,clippy` reported "Fixed tooling/src/verify.rs (1 fix)", passed, and left the file byte for byte as it was before the test.
