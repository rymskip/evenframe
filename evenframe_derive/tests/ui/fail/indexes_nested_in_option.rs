use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Customer {
    pub first_name: String,
}

#[derive(Debug, Clone, Evenframe)]
#[indexes(customer_search(fields("customer.first_name"), fulltext))]
pub struct Deal {
    pub id: String,
    pub customer: Option<Customer>,
}

fn main() {}
