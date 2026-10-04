use evenframe_derive::SurrealValue;

#[derive(SurrealValue)]
#[serde(untagged, deny_unknown_fields)]
enum Object {
    Left {
        #[serde(alias = "right")]
        left: u32,
    },
    Right {
        right: u32,
    },
}

fn main() {}
