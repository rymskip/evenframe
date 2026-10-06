use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Profile {
    #[surreal(serialize_with = "write_name")]
    pub name: String,
}

fn main() {}
