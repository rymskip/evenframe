use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    #[hnsw(dimension = 3, name = "post_vector")]
    #[diskann(dimension = 3, name = "post_vector")]
    pub embedding: Vec<f32>,
}

fn main() {}
