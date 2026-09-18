use evenframe::Evenframe;

#[derive(Evenframe)]
pub struct User {
    pub id: String,
    pub name: String,
    pub address: Address,
    pub role: Role,
}

#[derive(Evenframe)]
pub struct Address {
    pub city: String,
}

#[derive(Evenframe)]
pub enum Role {
    Admin,
    Member,
}
