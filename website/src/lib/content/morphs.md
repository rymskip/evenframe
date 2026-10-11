## Normalize before validation

`#[morphs(...)]` rewrites a value into canonical form without changing its type. All morphs run in the order written, before validators.

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
pub struct Label {
    #[morphs(trim, collapse_whitespace, lower)]
    #[validators(non_empty, max_length = 80)]
    pub value: String,
}
```

## String morphs

| Morph | Behavior |
| --- | --- |
| `trim` | Remove leading and trailing whitespace |
| `collapse_whitespace` | Replace each whitespace run with one space |
| `lower`, `upper` | Change case |
| `capitalize` | Uppercase the first character |
| `normalize`, `normalize_nfc` | Unicode NFC normalization |
| `normalize_nfd`, `normalize_nfkc`, `normalize_nfkd` | Other Unicode normalization forms |

## Number and array morphs

| Morph | Applies to | Behavior |
| --- | --- | --- |
| `round = 2` | `f64` | Round to decimal places, with JavaScript-style half-up behavior |
| `clamp = ("0", "100")` | Integers, `f64`, decimal text in `String` | Limit a value to a range |
| `sort` | Vectors of strings, characters, integers, booleans | Order elements; strings use code-point order |
| `unique` | Those same vector element types | Keep the first occurrence of each value |

Quantities and percentages may use floats. Monetary values should use decimal types with an explicit foreign mapping.

## Across the pipelines

Rust deserialization and validated newtype construction normalize before checking. ArkType and Effect emit transformations. Macroforge writes normalization in its endec annotations; FlatBuffers describes it as a normalize attribute. Mock generation also accounts for morphs.

The database stores the normalized value. Morphs add no `ASSERT`, since normalization is not a constraint on the original input.

A morph on an unsupported type is a compile error. Writing a morph inside `#[validators(...)]` is also rejected with guidance to move it to `#[morphs(...)]`.
