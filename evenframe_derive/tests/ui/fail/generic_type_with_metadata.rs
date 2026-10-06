use evenframe_derive::Evenframe;
use serde::Serialize;

/// A registry entry names one concrete type.
#[derive(Serialize, Evenframe)]
pub struct Page<T> {
    pub items: Vec<T>,
}

fn main() {}
