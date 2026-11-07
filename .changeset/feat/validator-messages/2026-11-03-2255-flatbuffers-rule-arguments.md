# fix(flatbuffers): escape a rule's argument as a literal of its own

- **Date:** 2026-11-03 22:55
- **Commit:** db0d6d6
- **Area:** Typesync (FlatBuffers)

## Problem

A validator argument holding a backslash or a double quote produced a `validate` attribute that said something else. A prefix ending in `"` came out as `startsWith(\"bk`${x}\"\")`, which reads back as `startsWith("bk`${x}"")`, a literal cut short. Every backslash in a pattern lost one escape: the `symbol` format's `\[` and the `relative_path` format's `\x00` reached the rule text with a single backslash, which the rule's own literal then read as its escape, not the pattern's.

## Cause

Each argument was escaped once and wrapped in already-escaped quotes (`\"...\"`), so one escape stood for two layers: the argument's literal inside the rule, and the rule list inside the attribute's string.

## Introduced

d9d1a05 (2025-12-09), `Add FlatBuffers and Protocol Buffers schema generation support`. It wrote every quoted argument as `name(\"{}\")` around `escape_for_fbs`, and later rules (formats, dates, big numbers, durations) followed its form.

## Fix

A rule's argument is a plain double-quoted literal with `\` and `"` escaped, and the whole rule list is escaped again when it becomes the attribute's string, as the `normalize` steps already were. One `quoted` helper and one `pattern_rule` helper write every argument.

## Verified

- New unit test `an_argument_is_escaped_as_a_literal_and_again_as_the_attribute`, asserting both the rule text and the attribute.
- The `flatbuffers_format_arguments` and `flatbuffers_validator_filters` snapshots, the only ones holding such an argument, now carry the double escape; flatc compiles every generated schema in `pixi run verify`.
