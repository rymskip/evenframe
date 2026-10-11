## Generate types during builds

The runtime dependency provides the derive; the build dependency provides scanning and the selected output generators:

```toml
[dependencies]
evenframe = { version = "0.6", default-features = false }
serde = { version = "1", features = ["derive"] }

[build-dependencies]
evenframe = { version = "0.6", default-features = false, features = ["build-typesync", "arktype", "effect"] }
```

Configure `evenframe.toml`:

```toml
[typesync]
outputs = [
  { kind = "arktype", dir = "./web/src/generated/arktype" },
  { kind = "effect", dir = "./web/src/generated/effect", mode = "per_file" },
]
```

Add `build.rs`:

```rust
fn main() -> Result<(), evenframe::error::EvenframeError> {
    evenframe::build::typesync()?;
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=evenframe.toml");
    Ok(())
}
```

The configuration is found from `CARGO_MANIFEST_DIR` upward. The scan covers that configuration's project root, finds derived source types, and generates the configured files. Unchanged files are left untouched. For configuration built in code, pass a `ScanConfig` to `evenframe::build::typesync_with`.

## Dump the schema offline

Enable `build-schemadump`, then call `evenframe::build::schemadump()`. It writes `.evenframe/surql/schema.surql` without a database connection or SDK client.

`build-fullstack` enables both build-script APIs. Add the type generators you intend to run alongside it.

```rust
fn main() -> Result<(), evenframe::error::EvenframeError> {
    evenframe::build::typesync()?;
    evenframe::build::schemadump()?;
    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=evenframe.toml");
    Ok(())
}
```

The schema dump requires a `[schemasync]` section. Connection environment variables do not need to be set for an offline dump.

## Keep database changes explicit

Build scripts run under `cargo check`, rust-analyzer, and CI as well as normal builds. They generate files but do not apply changes to a database. Use the CLI or the schemasync library API for live synchronization.
