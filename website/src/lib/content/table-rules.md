## A table is a struct with an id

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
pub struct User {
    pub id: String,
    #[unique]
    #[validators(email)]
    pub email: String,
}
```

The table configuration combines field types, validators, storage naming, indexes, permissions, events, and mock settings.

## Indexes

Use field attributes for a single top-level field: `#[unique]`, `#[fulltext(...)]`, `#[hnsw(...)]`, or `#[diskann(...)]`. Composite indexes and nested paths belong in one container `#[indexes(...)]` declaration.

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[indexes(user_message(fields(user, message), unique))]
pub struct Message {
    pub id: String,
    pub user: String,
    pub message: String,
}
```

Container `#[index(...)]` has been replaced by `#[indexes(...)]`. A simple single-field unique index belongs on its field with `#[unique]`.

Full-text indexes reference a configured analyzer. Vector indexes describe the dimension, distance, and the algorithm's options. Their declarations are validated by the attribute parser.

## Permissions and events

`#[permissions(...)]` sets table permissions. Each of `select`, `create`, `update`, and `delete` takes a SurrealQL condition in parentheses. `all` sets every action that has no condition of its own, and an action neither covers is `FULL`. Events are whole SurrealQL statements attached to the table:

```rust
#[permissions(select("true"), update("$auth.id = id"), delete("false"))]
#[event("DEFINE EVENT changed ON TABLE user WHEN $before != $after THEN { RETURN true }")]
```

Empty event statements are rejected. These rules are part of schema generation and can also be supplied by output-rule plugins.

## Relationships

Use `RecordLink<T>` for a field that can hold an SDK record id or a fetched record. Its TypeScript id representation needs a foreign mapping.

`#[relation]` on a struct makes its table a relation table. The record links in its `in` and `out` fields name the tables it connects. The edge name recorded in the type's metadata is the table's name, `wrote` here, unless `#[relation(edge_name = "authored")]` (or `name`) gives another.

```rust
#[derive(Debug, Clone, serde::Serialize, evenframe::Evenframe)]
#[relation]
pub struct Wrote {
    pub id: String,
    pub r#in: RecordLink<User>,
    pub out: RecordLink<Message>,
}
```

`#[edge(...)]` on a field of another table marks it as following an edge, such as `#[edge(name = "post_author", from = "Post", to = "User", direction = "from")]`. `name` (or `edge_name`), `from`, and `to` are required; `direction` is `from`, `to`, or `both`. An edge field is not defined on its own table and gets no mock value. Its name, endpoints, and direction are recorded in the type's metadata.

## Field definitions

Generated `TYPE`, `ASSERT`, and fallback `DEFAULT` follow the stored shape and validators. A fallback value that fails a check is not emitted as a default.

`#[define_field_statement(...)]` sets parts of one field's definition, each value in parentheses: `data_type`, `default`, `default_always`, `value`, `assert`, `computed`, `comment`, and `select_permissions`, `create_permissions`, and `update_permissions` take SurrealQL in a string, while `readonly`, `flexible`, and `should_skip` take `true` or `false`.

```rust
#[define_field_statement(readonly(true), comment("Set once"))]
pub created_by: String,
```

For detailed Rust configuration types and attribute options, use the API reference on docs.rs and the parser diagnostics for the declaration being written.
