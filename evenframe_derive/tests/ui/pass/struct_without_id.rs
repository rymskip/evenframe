use evenframe_derive::Evenframe;

/// Struct without id field - should be an object (not a table)
#[derive(Debug, Clone, Evenframe)]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip_code: String,
}

fn main() {
    println!("Test passed");
}
