## Configure your outputs

Use `output` for one generator or `outputs` for several. Each output owns its directory:

```toml
[typesync]
ts_names = "respect_serde"
outputs = [
  { kind = "arktype", dir = "./web/src/generated/arktype" },
  { kind = "effect", dir = "./web/src/generated/effect", mode = "per_file" },
]
```

Run `evenframe typesync` to write all configured outputs. Use `--formats arktype,effect` to select kinds or `--skip protobuf` to omit a kind. Selection does not add an output that is absent from the configuration.

## Available generators

| Kind | Output | Default file | Cargo feature |
| --- | --- | --- | --- |
| `arktype` | ArkType types, checks, transformations, brands | `arktype.ts` | `arktype` |
| `effect` | Effect schemas and encoded/decoded types | `bindings.ts` | `effect` |
| `macroforge` | TypeScript interfaces and macro annotations | `macroforge.ts` | `macroforge` |
| `protobuf` | Protocol Buffers schema | `schema.proto` | `protobuf` |
| `flatbuffers` | FlatBuffers schema | `schema.fbs` | `flatbuffers` |

The installed CLI includes all generators. In library and build-script dependencies, select their Cargo features explicitly. `typesync-all` enables every generator.

## ArkType and Effect

Both generators model serde shapes, validators, morphs, branded newtypes, and text codecs. Effect foreign types additionally describe their encoded representation. Install the corresponding runtime library in the application that consumes generated definitions.

## Protocol Buffers and FlatBuffers

Binary schema outputs support output-specific configuration. A Protobuf output accepts `package` and `import_validate`; a FlatBuffers output accepts `namespace`. Source types must be representable by the selected format: unsupported shapes or missing foreign mappings produce diagnostics.

```toml
[typesync]
outputs = [
  { kind = "protobuf", dir = "./generated/protobuf", package = "example.app", import_validate = true },
  { kind = "flatbuffers", dir = "./generated/flatbuffers", namespace = "Example.App" },
]
```

FlatBuffers writes a field's validators as rules in its `validate` attribute, such as `minLength(3), excludes("/")`. A text argument is a double-quoted literal with `\` and `"` escaped, and the rule list is escaped again as the attribute's string. A format argument becomes the pattern it stands for: `pattern("...")` where the value must match it, and `notPattern("...")` for `excludes`.

## Source ownership

Generated files are outputs. Make changes in Rust attributes, generator configuration, or plugins and regenerate. Editing a generated file directly will lose the change on the next sync.
