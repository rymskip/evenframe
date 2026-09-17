use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    pub title: String,
    #[fulltext(bm25(k1 = 1.2))]
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
