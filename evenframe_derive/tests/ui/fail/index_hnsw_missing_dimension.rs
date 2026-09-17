use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    #[hnsw(dist = "cosine")]
    pub embedding: Vec<f32>,
}

fn main() {}
