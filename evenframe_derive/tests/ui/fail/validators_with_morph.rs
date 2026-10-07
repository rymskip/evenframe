use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Profile {
    #[validators(trim, non_empty)]
    pub name: String,
}

fn main() {}
