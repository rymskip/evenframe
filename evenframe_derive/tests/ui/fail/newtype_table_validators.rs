use evenframe_derive::Evenframe;

/// A newtype's validators check its value, so a table's SurrealQL validator
/// is not one of them.
#[derive(Evenframe)]
#[validators(custom = "$value != ''")]
pub struct NonEmptyString(String);

fn main() {}
