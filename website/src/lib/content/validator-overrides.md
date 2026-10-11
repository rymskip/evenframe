## Replace a pipeline's validator list

A field, tuple element, or newtype may need a check that only one pipeline understands. `typesync` and `schemasync` overrides replace that pipeline's list outright.

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
pub struct Member {
    pub id: String,
    #[validators(non_empty)]
    #[typesync(validators(regex_literal = format(custom = "/^\\p{Lu}/u")))]
    #[schemasync(validators(min_length = 2))]
    pub name: String,
}
```

## TypeScript overrides

`#[typesync(validators(...))]` changes the generated outputs. Custom regex patterns here are JavaScript regex literals, including delimiters and flags. Supported flags include `i`, `m`, `s`, and `u` or `v`.

## Schema overrides

`#[schemasync(validators(...))]` changes schema assertions and mock data checks. Custom patterns here are Rust regex expressions, matching what SurrealDB's `string::matches` reads.

## Rust keeps the shared list

The Rust read runs only `#[validators(...)]`. Overrides do not add Rust validation. A field with overrides alone has no shared validator list to execute in Rust.

Mock records must meet the schema's list and, when different, the TypeScript list. An override is a replacement, not an extra validator appended to the shared list.
