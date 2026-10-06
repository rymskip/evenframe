use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[surreal(tag = "kind")]
pub struct Profile {
    pub name: String,
}

fn main() {}
