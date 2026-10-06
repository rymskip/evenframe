use evenframe_derive::Evenframe;

/// A tuple struct is written as an array of its fields.
#[derive(Evenframe)]
pub struct TupleStruct(String, i32);

fn main() {}
