use evenframe_derive::SurrealValue;

#[derive(SurrealValue)]
#[serde(untagged)]
enum Object {
    Left { left: Option<u32> },
    Right { right: Option<u32> },
}

fn main() {}
