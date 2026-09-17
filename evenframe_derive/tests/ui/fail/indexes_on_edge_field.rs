use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Customer {
    pub first_name: String,
}

#[derive(Debug, Clone, Evenframe)]
#[indexes(by_owner(fields(owner, title), unique))]
pub struct Deal {
    pub id: String,
    pub title: String,
    #[edge(name = "deal_owner", from = "Deal", to = "Customer", direction = "from")]
    pub owner: String,
}

fn main() {}
