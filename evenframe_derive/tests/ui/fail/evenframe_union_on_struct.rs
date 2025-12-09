use evenframe_derive::EvenframeUnion;

/// EvenframeUnion can only be used on enums
#[derive(EvenframeUnion)]
pub struct BadStruct {
    name: String,
}

fn main() {}
