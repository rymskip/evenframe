# feat(validators): a message the validators of a value report

- **Date:** 2026-11-03 17:53
- **Commit:** 4e20326
- **Area:** Validators, typesync

## Problem

A refused value reported what each validator expected, such as `must be at least 3 characters`, and nothing let a type say what its value must be. A newtype that combines several checks into one rule, such as a file name, gave the user a list of regex and length failures instead of one sentence.

## Fix

`#[validators(..., message = "...")]` sets one message for every validator in the list, on a field or on a newtype. The derive reports it in place of each validator's own description. ArkType reports it through the definition's `message` (which also covers a value of the wrong kind), Effect through each filter's `message`, and Macroforge through each validator's `message`. SurrealDB assertions carry no message.

The parser refuses a second message on one value, a message with no validators beside it, an empty or non-string message, and a message in a pipeline override. A newtype's container and field attributes are read together, in the derive and in the scanner, so the two cannot each set one.

## Verified

- Parser tests for each refusal; derive tests `a_refused_value_reports_its_message` and `a_refused_newtype_reports_its_message`.
- The new `validator_messages` spec, snapshotted for every generator.
- `generated_check` runtime cases requiring ArkType and Effect to report the message for a field and for a newtype.
- `pixi run verify`: every step passed, before the fixture's newtype moved from `excludes` to a pattern; `pixi run test-generated` then passed every runtime case on the committed tree.
