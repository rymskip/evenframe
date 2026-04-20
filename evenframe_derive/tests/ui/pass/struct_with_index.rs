use evenframe_derive::Evenframe;

/// Struct with two struct-level `#[index(...)]` attributes:
/// - a composite UNIQUE index on (user, message)
/// - a single-column non-unique index on created_at
#[derive(Debug, Clone, Evenframe)]
#[index(fields(user, message), unique)]
#[index(fields(created_at))]
pub struct Reaction {
    pub id: String,
    pub user: String,
    pub message: String,
    pub emoji: String,
    pub created_at: String,
}

fn main() {
    println!("Test passed");
}
