use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[morphs(trim)]
pub struct Code {
    pub pin: String,
}

fn main() {}
