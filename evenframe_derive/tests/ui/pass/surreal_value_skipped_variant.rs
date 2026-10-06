use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub enum Status {
    Active,
    #[serde(skip)]
    Internal,
}

fn main() {}
