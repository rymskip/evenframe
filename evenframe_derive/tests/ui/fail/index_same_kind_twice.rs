use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    #[hnsw(dimension = 3)]
    #[hnsw(dimension = 3, m = 8)]
    pub embedding: Vec<f32>,
}

fn main() {}
