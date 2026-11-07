## Declare field constraints

Validators use snake_case attribute names. A check without arguments is bare; a check with a value uses `=`:

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
pub struct Signup {
    #[validators(email)]
    pub email: String,
    #[validators(
        min_length = 8,
        includes = format(uppercase),
        includes = format(digit),
        includes = format(symbol)
    )]
    pub password: String,
}
```

When validator kinds share a name, qualify it: `string_validator(min_length = 3)`. The path form `StringValidator::MaxLength(80)` is also accepted.

## Where checks run

- **Rust input:** generated serde deserialization normalizes and validates fields, collecting field validation failures.
- **Rust values:** `Validate::validate()` checks values built in code and nested values.
- **TypeScript:** each output describes the corresponding checks.
- **SurrealDB:** schema assertions check supported constraints at stored positions.

Do not also derive `Deserialize` on a struct whose fields carry validators or morphs. The generated deserializer uses serde's attributes through a shadow type.

Container validators belong on newtypes. Put checks on fields or tuple elements for other struct and enum shapes.

## Text patterns

`starts_with`, `ends_with`, `includes`, and `excludes` accept quoted text or a format. Formats include `uppercase`, `lowercase`, `digit`, `symbol`, `relative_path`, and `slug`, along with the mock-data formats. `excludes` refuses a value holding the text, or any match of the format, anywhere in it.

A custom format used in these positional checks may not include its own `^` or `$`: the validator anchors it at the appropriate position.

## Messages

A failed validator reports what it expected, such as `must be at least 8 characters`. A `message` in the same `#[validators(...)]` replaces that for every validator it lists:

```rust
#[validators(
    non_empty,
    regex_literal = format(custom = r"^[^/]+$"),
    message = "must be one path segment"
)]
pub name: String,
```

The Rust check reports `name: must be one path segment`, and each TypeScript output reports the same text in its own form. ArkType reports it for a value of the wrong kind as well; the others keep their own type errors. A value has one message, so a newtype's container and its field cannot both set one, and a pipeline override cannot set one at all. SurrealDB assertions carry no message.

## Portable regular expressions

A shared `regex_literal = format(custom = "...")` must mean the same thing in Rust regex and JavaScript RegExp. The portable subset rejects `\d`, `\w`, `\s`, `\b`, Unicode property escapes, `.`, inline flags, and class-set operations. Use explicit ranges such as `[0-9]`. Negated classes are accepted only directly under `*` or `+`.

If one pipeline needs different regex capabilities, use a pipeline override instead of weakening the shared check.

## Schema limitations

Recursive structs, open shapes, and untagged variants that cannot be stated unambiguously in SurrealQL can prevent some nested assertions. Schemasync warns where a check cannot be emitted. A fallback default that fails a validator is omitted, leaving the field required.

Some non-native assertions use embedded JavaScript. `schemasync.mock_gen_config.scripting_asserts` controls their emission; servers executing them need scripting enabled.
