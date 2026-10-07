use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Order {
    #[morphs(trim)]
    pub quantity: u32,
}

fn main() {}
