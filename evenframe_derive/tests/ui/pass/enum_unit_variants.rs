use evenframe_derive::Evenframe;

/// Enum with unit variants
#[derive(Debug, Clone, Evenframe)]
pub enum Status {
    Active,
    Inactive,
    Pending,
}

fn main() {
    println!("Test passed");
}
