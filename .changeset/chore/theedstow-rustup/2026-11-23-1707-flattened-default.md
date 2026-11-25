# fix(default): default a flattened field beside its siblings

- **Date:** 2026-11-23 17:07
- **Commit:** this commit
- **Area:** Schemasync

## Problem

An embedded struct with a flattened map defaulted to `{ label: '', rest: {} }`: the map under
its own key. Serde reads `rest` back as one more entry of the flattened map, whose value `{}` is
not the map's value type, so a record holding the default could not be decoded.

## Cause

The default object listed every field under its own name, a flattened one included, where serde
writes a flattened field's keys beside its siblings.

## Introduced

cf9559c (2026-10-06, "feat(core): newtypes and every serde wire shape") kept a flattened map or
enum as a field of its struct, which the default object then named.

## Fix

A flattened field adds its default's own keys to the object: none for an empty map, and an enum's
default keys beside the struct's other fields.

## Verified

A new unit test shows an embedded struct with a flattened map defaults to `{ label: '' }`, and
the default tests pass. `pixi run verify` passed every step (run `20261123T214351Z`).
