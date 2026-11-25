# fix(schemasync): define functions before the tables whose permissions call them

- **Date:** 2026-11-22 23:58
- **Commit:** this commit
- **Area:** Schemasync, schema dump, docs

## Problem

Against SurrealDB 4, a schemasync that changed a table whose `PERMISSIONS` clause calls a
function failed for that table whenever the stored function had a body SurrealDB refuses there,
such as `any` passed a variable: "The PERMISSIONS clause must not have effects". Correcting the
function did not help, since schemasync redefined the tables before the functions, so every
changed table was still checked against the old body.

## Cause

SurrealDB 4 checks a changed table's `PERMISSIONS` against the stored body of each function it
calls. Schemasync applied accesses, analyzers, tables, then functions, on the grounds that a
function's `record<table>` parameters needed their tables first, which SurrealDB 4 does not
require.

## Introduced

a04c692 (2026-03-11, "Fix event definition execution in schemasync") put the functions after the
tables, which was correct against SurrealDB 3.

## Fix

Schemasync, its in-memory comparison schema and the schema dump all apply accesses, functions,
analyzers, then tables. Functions going first also covers an analyzer that preprocesses with one,
so `analyzers_reference_functions` and its special case are gone. The docs give the new order.

## Verified

`pixi run verify` passed all steps (run `20261123T054427Z`). A new e2e test stores a table and a
function passing `any` a variable, then shows that redefining the changed table before the
function is refused, and that the dump, functions first, applies.
