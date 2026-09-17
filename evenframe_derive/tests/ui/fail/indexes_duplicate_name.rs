use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[indexes(by_body(fields(title, body)), by_body(fields(body, title)))]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
