use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Customer {
    pub first_name: String,
    pub last_name: String,
}

#[derive(Debug, Clone, Evenframe)]
#[indexes(customer_search(fields("customer.first_name"), fulltext(fields(x))))]
pub struct Deal {
    pub id: String,
    pub title: String,
    pub customer: Customer,
}

fn main() {}
