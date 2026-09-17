use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    #[fulltext(name = "1bad-name")]
    pub body: String,
}

fn main() {}
