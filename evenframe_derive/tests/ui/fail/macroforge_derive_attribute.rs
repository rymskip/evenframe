use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[macroforge_derive(Default)]
pub struct Profile {
    pub name: String,
}

fn main() {}
