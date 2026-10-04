use evenframe_derive::SurrealValue;

#[derive(SurrealValue)]
#[serde(untagged)]
enum Object {
    Basic { quantity: u32 },
    Detailed { quantity: u32, label: String },
}

fn main() {}
