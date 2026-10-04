use evenframe_derive::SurrealValue;

#[derive(SurrealValue)]
#[serde(untagged)]
enum Amount {
    Count(i64),
    Total(u64),
}

fn main() {}
