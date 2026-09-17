use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[indexes(by_body(fields(body), search))]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
