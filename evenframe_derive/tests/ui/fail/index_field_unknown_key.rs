use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    pub title: String,
    #[fulltext(analyzer = "en", fields(body))]
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
