use evenframe_derive::Evenframe;

/// Tuple struct - Evenframe doesn't support tuple structs
#[derive(Evenframe)]
pub struct TupleStruct(String, i32);

fn main() {}
