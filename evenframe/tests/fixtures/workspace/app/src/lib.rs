mod address;

use address::Address;
use evenframe::Evenframe;

#[derive(Evenframe)]
pub struct Deal {
    pub id: String,
    pub address: Address,
}
