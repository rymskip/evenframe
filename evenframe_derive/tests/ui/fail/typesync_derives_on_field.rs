use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Profile {
    #[typesync(macroforge(derives = [Default]))]
    pub name: String,
}

fn main() {}
