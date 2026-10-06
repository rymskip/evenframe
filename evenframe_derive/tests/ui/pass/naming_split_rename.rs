use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Person {
    #[serde(rename(serialize = "fullName", deserialize = "name"))]
    pub name: String,
}

fn main() {}
