use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[serde(tag = "kind")]
pub enum Shape {
    Circle(f64),
}

fn main() {}
