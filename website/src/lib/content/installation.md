## Install the CLI

```sh
cargo install evenframe
evenframe init
```

Run the commands from your project's directory. `init` creates an `evenframe.toml` and, when the directory has none, a `.env` with SurrealDB connection defaults. `evenframe init --minimal` starts with minimal configuration, and `--force` overwrites an existing file.

## Add the derive

Use a small dependency when the CLI or a build script will generate your files:

```toml
[dependencies]
evenframe = { version = "0.6", default-features = false }
serde = { version = "1", features = ["derive"] }
```

Without features, the derive still emits `Validate`, validated deserialization where needed, and the `EvenframeTable` marker.

## Add runtime metadata

```toml
[dependencies]
evenframe = { version = "0.6", default-features = false, features = ["metadata"] }
serde = { version = "1", features = ["derive"] }
```

The `metadata` feature registers each derived type's description and adds configuration functions. It does not need a database client.

## Add build-time generation

```toml
[build-dependencies]
evenframe = { version = "0.6", default-features = false, features = ["build-typesync", "arktype"] }
```

Select generators explicitly: `arktype`, `effect`, `macroforge`, `protobuf`, or `flatbuffers`. `typesync-all` enables all five. `build-typesync` enables scanning and generation infrastructure, not the generators themselves.

## Database value types

Enable `surrealdb-types` for `RecordLink` and the derived `SurrealValue` implementation without bringing in the SDK's client. Live synchronization uses `schemasync`; mock generation uses `mockmake`.

The default feature is `cli`. Library and build-dependency setups should disable it and choose the features they need.
