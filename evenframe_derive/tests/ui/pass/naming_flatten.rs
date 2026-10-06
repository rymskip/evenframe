use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Address {
    pub city: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Person {
    pub name: String,
    #[serde(flatten)]
    pub address: Address,
}

fn main() {}
