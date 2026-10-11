## Single file or per type

Single-file output is the default. Effect and Macroforge also support `mode = "per_file"`; ArkType, Protobuf, and FlatBuffers do not.

```toml
[typesync]
output = { kind = "effect", dir = "./web/src/generated", mode = "per_file", barrel_file = true }
```

Each primary type gets its own file. Dependents used exclusively by that type are colocated. `barrel_file = true` adds an index module.

## File settings

| Key | Default | Alternative |
| --- | --- | --- |
| `file` | Generator's standard file name | Custom single-file name |
| `file_naming` | `kebab` | `pascal`, `snake`, `camel` |
| `file_extension` | `.ts` | For example `.svelte.ts` |
| `array_style` | `shorthand` | `generic` for `Array<Type>` |
| `import_extension` | `bare` | `js` for compiled package imports |
| `barrel_file` | `false` | `true` |

With `import_extension = "js"`, emitted imports name the transpiled extension so packaged output resolves under strict Node/Vite resolution. TypeScript resolves the `.js` specifier to its `.ts` source before compilation.

## Struct variants

Named enum struct variants are emitted as separate types by default. If different enums use the same variant name for different payloads, choose inline variants:

```toml
[typesync]
struct_variants = "inline"
output = { kind = "effect", dir = "./web/src/generated" }
```

## Name collisions

Two source files that define a type of the same name stop generation by default. Rename one of them, or set `collision_strategy = "auto_rename"` under `[typesync]` to prefix the later type with its module's name, so `PaymentMethod` in the `invoice` module becomes `InvoicePaymentMethod`. Shared workspace types are generated once when their definitions agree.
