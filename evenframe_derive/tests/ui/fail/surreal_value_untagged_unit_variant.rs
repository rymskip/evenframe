use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[serde(untagged)]
pub enum Amount {
    Whole(i64),
    Unknown,
}

fn main() {}
