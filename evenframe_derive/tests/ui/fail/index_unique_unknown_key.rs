use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Account {
    pub id: String,
    #[unique(name = "account_email", fields(email))]
    pub email: String,
}

fn main() {}
