use evenframe_derive::Evenframe;

/// Struct with two struct-level `#[indexes(...)]` entries:
/// - a composite UNIQUE index on (user, message)
/// - a single-column non-unique index on created_at
#[derive(Debug, Clone, Evenframe)]
#[indexes(
    reaction_user_message(fields(user, message), unique),
    reaction_created_at(fields(created_at)),
)]
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
