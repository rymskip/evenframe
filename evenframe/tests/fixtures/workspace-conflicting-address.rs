use evenframe::Evenframe;

#[derive(Evenframe)]
pub struct Account {
    pub id: String,
    pub home: Address,
}

#[derive(Evenframe)]
pub struct Address {
    pub street: String,
}
