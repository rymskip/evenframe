## Configure Macroforge

```toml
[typesync]
output = { kind = "macroforge", dir = "./web/src/generated", mode = "per_file" }
```

A struct or enum gets `Decode` by default; a newtype gets no default derive. Set `default_derives` on the output to replace those defaults for types with no derives of their own:

```toml
[typesync]
output = { kind = "macroforge", dir = "./web/src/generated", default_derives = ["Default", "Encode", "Decode"] }
```

## Source derives and annotations

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[typesync(macroforge(derives = [Default, Encode, Decode]))]
pub struct Profile {
    #[typesync(annotation("@input({ label: \"Name\" })"))]
    pub name: String,
    #[typesync(macroforge(attributes = [endec(rename = "joined_at"), hidden]))]
    pub joined: String,
}
```

`macroforge(derives = [...])` sets derives on structs, enums, newtypes, or struct variants. Attributes become the JSDoc annotations Macroforge reads. Keys are written in camelCase, bare keys become `true`, and nested lists become objects. Values may be string, number, or boolean literals, or arrays.

`annotation("...")` writes its annotation text unchanged. Custom annotations require the consuming application's implementation.

## Import custom macros

Macroforge itself provides `Clone`, `Debug`, `Default`, `Decode`, `Encode`, `Hash`, `Ord`, `PartialEq`, and `PartialOrd`. An output's `macros` map identifies the package for every other derive:

```toml
[typesync]
output = { kind = "macroforge", dir = "./web/src/generated", macros = { Form = "@app/forms" } }
```

A custom derive with no configured package is an error. The generated import names the configured package; that package must export the macro.

## Validation and normalization

Validation, enum tagging, foreign-type formats, and codecs use endec annotations. Morphs become normalization settings. Validators without native Macroforge equivalents get helper functions beside the generated output, referenced through custom checks.

Newtypes use Macroforge's branded newtype representation. Rust serde attributes still determine serialization and enum tags.
