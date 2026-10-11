# fix(cli): check each project of a workspace against its own cache

- **Date:** 2026-10-11 14:51
- **Commit:** 0cb4471
- **Area:** CLI, check

## Problem

From a workspace root, `evenframe check` reported "No scan cache" and exited 1, even straight after the `evenframe validate --types-only` it told the user to run. A CI gate at the root could not pass.

## Cause

`check` loaded the nearest configuration as one project and looked for `.evenframe/cache.json` in its root. A workspace scan writes one cache per project, in each project's root, and none in the workspace root.

## Introduced

4b23f53 (2026-10-08), `feat(config): merge every config up the tree and sync workspaces in one run`. It moved typesync, schemasync, generate, mockmake and validate onto per-project scans and left `check` reading the nearest configuration.

## Fix

`check` reports every focused project against its own cache, as mockmake and schemasync do, and fails when any is missing or stale. A named project's report starts with `Project <name>` and its JSON carries `project`. `--json` from a workspace root is a list with one object per project; one project alone is the single object it was.

## Verified

New test `check_covers_every_project_of_a_workspace` in `evenframe/tests/cli.rs`. Before the fix its transcript showed the root reporting a missing cache after `validate --types-only`; after it, both projects report in sync with status 0, an edited project reports stale with status 1, and `check --json` inside one project is a single object. `cargo test -p evenframe`: 21 unit tests passed, and the other 11 CLI tests passed with their snapshots unchanged. `pixi run verify` passed every step with no warnings; its CLI step ran the 12 tests in `evenframe/tests/cli.rs`, all passing.
