use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[index(name = "1bad-name", fields(body))]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
