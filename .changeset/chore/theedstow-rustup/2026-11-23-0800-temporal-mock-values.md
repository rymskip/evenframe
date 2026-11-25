# fix(mockmake): cast a formatted mock value to a time or date column's type

- **Date:** 2026-11-23 08:00
- **Commit:** this commit
- **Area:** Mock data

## Problem

A foreign type stored as SurrealDB 4's `time` or `date`, such as chrono's `NaiveTime`, could not
be seeded: its mock value was a quoted string, which neither type accepts, so every generated row
was refused.

## Cause

A foreign type's mock strategy that names a format wrote the formatted value as a string literal
whatever the column's type. SurrealDB 4's SDK reads a `NaiveTime` only from a `time` value, so a
project storing one has to declare the column `time`.

## Introduced

2554300 (2026-04-06, "Emit separate JSDoc comments for enum variant annotations, fix mock format
fallback") wrote formatted strategies as text, which was correct against SurrealDB 3, which has no
`time` or `date` type.

## Fix

A formatted value bound for a column whose foreign type declares `time` or `date` is cast to that
type (`<time> '09:30:00'`); any other column still takes the text. The strategies were also
lifted into one function.

## Verified

Unit tests show a `time` strategy casts for a `time` column and stays text for a `string` one, and
that an unknown strategy falls through to the type's own shape.
