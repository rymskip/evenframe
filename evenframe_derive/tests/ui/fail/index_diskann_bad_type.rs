use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Post {
    pub id: String,
    pub title: String,
    pub body: String,
    #[diskann(dimension = 3, type = "f64")]
    pub embedding: Vec<f32>,
}

fn main() {}
