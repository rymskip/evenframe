# fix(cli): let test-plugin --changed-only be turned off

- **Date:** 2026-10-11 14:51
- **Commit:** d6f7787
- **Area:** CLI, test-plugin

## Problem

`evenframe test-plugin` only ever listed the types a plugin changed. Nothing could list the rest.

## Cause

`changed_only` was a boolean switch with `default_value = "true"`. Passing the flag set it to true, and leaving it out left it true.

## Introduced

25e6a71 (2026-04-06), `Switch CLI to RUST_LOG env-filter, add test-plugin command`. It added the flag in that form.

## Fix

The flag takes a value: `--changed-only false` shows every type, and the default stays true.

## Verified

`evenframe test-plugin --help` lists `--changed-only <CHANGED_ONLY>` with default `true` and possible values `true` and `false`. `pixi run verify` passed every step with no warnings.
