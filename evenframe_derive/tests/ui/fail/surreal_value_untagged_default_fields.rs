use evenframe_derive::SurrealValue;

#[derive(SurrealValue)]
#[serde(untagged)]
enum Object {
    Left {
        #[serde(default)]
        left: u32,
    },
    Right {
        #[serde(default)]
        right: u32,
    },
}

fn main() {}
