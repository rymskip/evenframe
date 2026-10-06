use evenframe_derive::Evenframe;

/// A tuple struct has no one value for its validators to check.
#[derive(Evenframe)]
#[validators(StringValidator::NonEmpty)]
pub struct Pair(String, i32);

fn main() {}
