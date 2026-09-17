use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    #[hnsw(dimension = 3, m = 128)]
    pub embedding: Vec<f32>,
}

fn main() {}
