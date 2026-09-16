use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[index(fields(embedding), diskann(dimension = 3, type = "f64"))]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
