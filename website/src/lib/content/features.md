## Start small

`evenframe` defaults to the `cli` feature set. Library consumers should use `default-features = false` and add what they need.

| Feature | Adds |
| --- | --- |
| `metadata` | Config functions and runtime registration |
| `surrealdb-types` | SDK value types, `RecordLink`, and derived SDK conversion without a client |
| `typesync` | Type-generation infrastructure; no generator selected |
| `arktype` | ArkType generator; implies typesync |
| `effect` | Effect generator; implies typesync |
| `macroforge` | Macroforge generator; implies typesync |
| `protobuf` | Protocol Buffers generator; implies typesync |
| `flatbuffers` | FlatBuffers generator; implies typesync |
| `typesync-all` | All five output generators |
| `build-typesync` | Scanning and `build::typesync()`; select generators separately |
| `build-schemadump` | Scanning and offline `build::schemadump()` |
| `build-fullstack` | Both build-script APIs |
| `schemasync` | Live SurrealDB schema synchronization |
| `surrealdb` | SurrealDB client |
| `mockmake` | Mock generation; implies schemasync |
| `wasm-plugins` | WebAssembly plugin runtime |
| `cli` | Features used by the binary, including all generators and mock/plugin support |
| `full` | Metadata, all generators, schemasync with mock data, plugins, and both build-script APIs |
| `dev-mode` | Development file logging; not part of `cli` or `full` |

## Derive only

```toml
evenframe = { version = "0.6", default-features = false }
```

Validation, validated deserialization, and table markers remain available. Metadata and generated files are separate concerns.

## Build-time types

```toml
[build-dependencies]
evenframe = { version = "0.6", default-features = false, features = ["build-typesync", "effect"] }
```

## Runtime metadata and database types

```toml
[dependencies]
evenframe = { version = "0.6", default-features = false, features = ["metadata", "surrealdb-types"] }
```

This selects runtime descriptions and SDK values without selecting live synchronization or the full client.
