use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[index(fields(body), fulltext(bm25(k1 = 1.2)))]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    pub embedding: Vec<f32>,
}

fn main() {}
