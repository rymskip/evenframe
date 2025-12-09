use evenframe_derive::Evenframe;

/// Basic struct with id field - should be a table
#[derive(Debug, Clone, Evenframe)]
pub struct User {
    pub id: String,
    pub name: String,
    pub age: i32,
}

fn main() {
    println!("Test passed");
}
