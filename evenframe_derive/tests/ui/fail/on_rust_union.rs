use evenframe_derive::Evenframe;

/// Evenframe cannot be used on Rust unions
#[derive(Evenframe)]
pub union BadUnion {
    i: i32,
    f: f32,
}

fn main() {}
