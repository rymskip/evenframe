//! Tables whose links can have nothing to point at, a link through a
//! persistable union, links nested in objects, lists, enums and maps, and
//! a table using every mock data coordination: an optional and a list link
//! to a table that generates no records, which itself links to another such
//! table.

use super::ecommerce::Product;
use evenframe::types::RecordLink;
use evenframe::{Evenframe, EvenframeUnion};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 0)]
pub struct SerialBatchBundle {
    pub id: String,
    pub label: String,
    pub ledger: RecordLink<BatchLedger>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 0)]
pub struct BatchLedger {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 3)]
pub struct Service {
    pub id: String,
    pub name: String,
}

/// Something a line item bills for: a product or a service.
#[derive(Debug, Clone, Serialize, EvenframeUnion)]
pub enum Billable {
    Product(Product),
    Service(Service),
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct BilledItem {
    pub id: String,
    pub description: String,
    pub billable: RecordLink<Billable>,
    pub serial_batch_bundle: Option<RecordLink<SerialBatchBundle>>,
    pub bundles: Vec<RecordLink<SerialBatchBundle>>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct BookingSlot {
    pub service: RecordLink<Service>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub enum BookingKind {
    WalkIn,
    Scheduled(RecordLink<Service>),
    Package { services: Vec<RecordLink<Service>> },
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 3)]
pub struct ServiceBooking {
    pub id: String,
    pub slot: BookingSlot,
    pub slots: Vec<BookingSlot>,
    pub kind: BookingKind,
    pub by_day: HashMap<String, RecordLink<Service>>,
    pub backup: Option<BookingSlot>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct ShiftLocation {
    pub label: String,
    pub floor: u32,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(
    n = 4,
    coordinate = [
        Coordination::InitializeEqual(["location.label", "badge_label"]),
        Coordination::InitializeSequential {
            field_names: ["starts_on", "ends_on"],
            increment: CoordinateIncrement::Days(7)
        },
        Coordination::InitializeSum {
            field_names: ["deposit", "balance"],
            total: 100.0
        },
        Coordination::InitializeDerive {
            source_field_names: ["first_name", "last_name"],
            target_field_name: "full_name",
            derivation: DerivationType::Concatenate(" ")
        },
        Coordination::InitializeCoherent(CoherentDataset::Address {
            city: "city",
            state: "state",
            zip: "zip",
            country: "country"
        }),
    ]
)]
pub struct Shift {
    pub id: String,
    pub location: ShiftLocation,
    pub badge_label: String,
    #[format(Date)]
    pub starts_on: String,
    #[format(Date)]
    pub ends_on: String,
    pub deposit: f64,
    pub balance: f64,
    pub first_name: String,
    pub last_name: String,
    pub full_name: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
}
