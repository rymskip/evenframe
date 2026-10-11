## Define a Rust type

Add the derive and serde dependencies from **Installation**, then place this definition in your crate's sources:

```rust
use evenframe::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Profile {
    #[morphs(trim)]
    #[validators(non_empty, max_length = 80)]
    pub name: String,
    #[validators(email)]
    pub email: String,
}
```

`Profile` has no `id`, so it is an embedded object. `Evenframe` generates deserialization for its validated fields. Do not also derive `Deserialize` on this type.

## Configure an output

Create `evenframe.toml` at the project root:

```toml
[typesync]
output = { kind = "arktype", dir = "./web/src/generated" }
```

This setup generates types only. It requires no `[schemasync]` section and no database connection.

## Generate and inspect

```sh
evenframe validate
evenframe typesync
```

The ArkType output is written to `web/src/generated/arktype.ts`. It describes the two string fields, including their validation constraints and the trim transformation.

## Check Rust input

When serde reads a `Profile`, the name is trimmed before its validators run. A malformed email or empty name is rejected. Every failing field is reported in the validation error.

Values created directly in Rust can be checked with the `Validate` trait:

```rust
use evenframe::validator::validate::Validate;

fn check_profile(profile: &Profile) -> Result<(), evenframe::validator::validate::ValidationErrors> {
    profile.validate()
}
```

## Make it automatic

For generation on each Cargo build, follow **Build scripts**. For live database synchronization, add an `id` to a table type and configure the database as described in **Schema synchronization**.
