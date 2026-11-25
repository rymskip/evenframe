# chore(toolchain): take rustup from theedstow and drop setup-rust

- **Date:** 2026-11-22 15:15
- **Commit:** this commit
- **Area:** Toolchain, pixi tasks

## Problem

`pixi run setup-rust` was a per-machine bootstrap that installed a global rustup from brads-forge,
linked `cargo`, `rustc` and the other proxies into `~/.cargo/bin`, and appended `RUSTUP_HOME` to
`~/.zshenv`. Every machine carried that state outside the repository, and a fresh run reinstalled
the brads-forge build.

## Cause

The brads-forge rustup ships only the `rustup` binary, so nothing provided the toolchain proxies.

## Fix

`rustup` comes from the theedstow channel, whose build ships the proxies in the environment and
leaves self-update to pixi, so the project environment's `cargo` runs the toolchain in
`rust-toolchain.toml` with no setup. `scripts/setup-rust.sh` and the `setup-rust` task are deleted.
Changing the channel list re-solves the whole environment, so the lock also moves every other
package to its newest build in range, among them deno 2.9.8 and hyperfine 1.21.0.

## Verified

With `RUSTUP_HOME` and `CARGO_HOME` unset in the shell, `pixi run` resolved `cargo` to the project
environment's proxy, took `RUSTUP_HOME` from `[activation.env]`, auto-installed the pinned 1.99.1
toolchain into it, and `cargo -V` reported 1.99.1.
