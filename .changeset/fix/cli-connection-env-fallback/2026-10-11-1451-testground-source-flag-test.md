# test(testground): remove the test for a source flag that no longer exists

- **Date:** 2026-10-11 14:51
- **Commit:** 07d7043
- **Area:** Testground

## Problem

`test_global_source_flag` asserted that the CLI's help mentions a source flag. The CLI has none, and the test passed on unrelated words in the help text.

## Cause

The assertion accepted `source`, `rust` or `flatbuffers` anywhere in the output.

## Introduced

dbf5c63 (2026-09-18), `0.5.0: correct mock data, strict config, unfinished work moved to branches`. It removed the `source` option that f3c0a4b (2025-12-27) had added with this test, and left the test.

## Fix

The test is deleted.

## Verified

`pixi run verify` passed every step with no warnings, the testground's e2e tests included.
