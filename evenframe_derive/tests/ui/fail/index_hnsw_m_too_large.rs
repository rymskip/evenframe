use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[index(fields(embedding), hnsw(dimension = 3, m = 128))]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
