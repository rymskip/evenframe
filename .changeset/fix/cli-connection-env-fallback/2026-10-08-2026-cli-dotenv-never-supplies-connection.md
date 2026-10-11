# fix(cli): a .env never supplies a connection argument

- **Date:** 2026-10-08 20:26
- **Commit:** 8e1bbda
- **Area:** CLI

## Problem

A project that names its own database in its config, such as a nested project config, was synced against whatever database an outer `.env` named, with no flag passed.

## Cause

The connection flags fell back to `SURREALDB_URL`, `SURREALDB_NS` and `SURREALDB_DB` through clap, and an early load put every `.env` in the config chain into the environment before parsing.

## Introduced

4b23f53 (2026-10-08), `feat(config): merge every config up the tree and sync workspaces in one run`. It merged configs up the tree, and its early load put every `.env` in the chain into the environment, where the connection flags' fallback found an outer project's database. The fallback itself dates from f3c0a4b (2025-12-27).

## Fix

The flags no longer read the environment and the early load is gone. Only the configuration that names a `.env` reads it: an explicit flag still outranks the config, and the config resolves its own variables.

## Verified

New CLI test `a_dotenv_never_supplies_a_connection_argument` with its snapshot.
