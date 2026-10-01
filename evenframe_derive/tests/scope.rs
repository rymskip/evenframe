//! The derive's generated code compiles whatever traits the deriving module
//! has in scope, here one whose `to_case` method applies to every type.

use evenframe::Evenframe;
use evenframe::traits::EvenframePersistableStruct;
use serde::Serialize;

mod casing {
    pub trait Casing {
        fn to_case(&self, case: u8) -> String;
    }

    impl<T> Casing for T {
        fn to_case(&self, case: u8) -> String {
            format!("case {case}")
        }
    }
}

use casing::Casing;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct ServiceOrder {
    pub id: String,
    pub note: String,
}

#[test]
fn a_table_derives_beside_another_casing_trait() {
    assert_eq!(
        ServiceOrder::static_table_config().table_name,
        "service_order"
    );
    assert_eq!("order".to_case(1), "case 1");
}
