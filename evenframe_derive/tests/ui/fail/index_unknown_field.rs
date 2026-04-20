use evenframe_derive::Evenframe;

/// `nonexistent` is not a field of the struct, so the derive must fail
/// with a clear `unknown field` compile error.
#[derive(Debug, Clone, Evenframe)]
#[index(fields(nonexistent))]
pub struct Reaction {
    pub id: String,
    pub user: String,
    pub message: String,
}

fn main() {}
