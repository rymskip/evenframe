use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Person {
    pub name: String,
    #[serde(skip_serializing)]
    pub password: String,
}

fn main() {}
