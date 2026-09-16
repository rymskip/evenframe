use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[index(fields(body), count)]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
