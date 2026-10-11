## TypeScript field names

By default, TypeScript fields use camelCase. Explicit naming takes precedence in this order:

1. A field's `#[evenframe(ts_name = "snake_case")]`.
2. The container's `#[evenframe(all_ts_names = "snake_case")]`.
3. Serde's `rename`, `rename_all`, or `rename_all_fields`.
4. The configured fallback naming policy.

Evenframe casing attributes convert the Rust field name to the chosen style.

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[evenframe(all_ts_names = "snake_case")]
pub struct Profile {
    pub display_name: String,
}
```

## Preserve Rust and serde names

For an application that keeps backend names throughout the frontend:

```toml
[typesync]
ts_names = "respect_serde"
output = { kind = "arktype", dir = "./web/src/generated" }
```

Fields without an Evenframe override keep serde's exact names, including unchanged Rust snake_case names.

## Supported casing styles

`lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, and `SCREAMING-KEBAB-CASE` are accepted.

An `all_ts_names` on an enum or named variant changes payload field names, not variant names. Variant names and tag/content keys always follow serde.

## Database names

TypeScript naming does not change Rust serialization or database keys, and neither does `ts_names`. A stored key is the Rust field name unless `#[surreal(rename = "...")]` on the field or `#[surreal(rename_all = "...")]` on its container renames it. Stored variant names follow the same rule.

Serde renames reach JSON and TypeScript only, so one type can be camelCase for the frontend and snake_case in the database. To store serde's names as well, state the rule for both:

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[serde(rename_all = "camelCase")]
#[surreal(rename_all = "camelCase")]
pub struct Membership {
    pub id: String,
    pub display_name: String,
}
```

Surreal casing preserves letters of any script. The SDK's casing splits some non-ASCII words differently; ASCII names are stored identically. Use Evenframe's derived SDK conversion when you need the exact shape its schema defines.
