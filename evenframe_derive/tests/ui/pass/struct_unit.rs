use evenframe_derive::Evenframe;

/// serde writes a unit struct as null, which the database stores.
#[derive(Evenframe)]
pub struct UnitStruct;

fn main() {}
