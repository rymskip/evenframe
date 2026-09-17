use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
#[indexes(by_email(fields(email), unique))]
pub struct Account {
    pub id: String,
    pub email: String,
}

fn main() {}
