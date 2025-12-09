use evenframe_derive::Evenframe;

/// Enum with mixed variant types
#[derive(Debug, Clone, Evenframe)]
pub enum Message {
    Quit,
    Move { x: i32, y: i32 },
    Write(String),
    ChangeColor(i32, i32, i32),
}

fn main() {
    println!("Test passed");
}
