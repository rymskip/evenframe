use axum::{Json, response::IntoResponse};
use serde_json::json;

use evenframe::types::RecordLink;
use evenframe::wrappers::EvenframeRecordId;
use std::collections::HashMap;

use crate::models::billing::{
    Billable, BookingKind, BookingSlot, Service, ServiceBooking, Shift, ShiftLocation,
};
use crate::models::{Product, ProductCategory};

/// List what line items can bill for (mock data)
pub async fn list_billables() -> impl IntoResponse {
    let billables = vec![
        Billable::Product(Product {
            id: "product:1".to_string(),
            name: "Laptop".to_string(),
            description: "A great laptop product".to_string(),
            price: 999.99,
            stock_quantity: 100,
            category: ProductCategory::Electronics,
            image_url: None,
            is_available: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }),
        Billable::Service(Service {
            id: "service:1".to_string(),
            name: "Installation".to_string(),
        }),
    ];

    Json(json!({
        "data": billables,
        "count": billables.len()
    }))
}

fn service_link(id: &str) -> RecordLink<Service> {
    RecordLink::Id(EvenframeRecordId::from(id.to_string()))
}

/// List service bookings (mock data)
pub async fn list_bookings() -> impl IntoResponse {
    let slot = BookingSlot {
        service: service_link("service:1"),
        note: "Morning".to_string(),
    };
    let bookings = vec![
        ServiceBooking {
            id: "service_booking:1".to_string(),
            slot: slot.clone(),
            slots: vec![slot.clone()],
            kind: BookingKind::Scheduled(service_link("service:1")),
            by_day: HashMap::from([("monday".to_string(), service_link("service:1"))]),
            backup: None,
        },
        ServiceBooking {
            id: "service_booking:2".to_string(),
            slot: slot.clone(),
            slots: vec![],
            kind: BookingKind::Package {
                services: vec![service_link("service:1")],
            },
            by_day: HashMap::new(),
            backup: Some(slot.clone()),
        },
        ServiceBooking {
            id: "service_booking:3".to_string(),
            slot,
            slots: vec![],
            kind: BookingKind::WalkIn,
            by_day: HashMap::new(),
            backup: None,
        },
    ];

    Json(json!({
        "data": bookings,
        "count": bookings.len()
    }))
}

/// List shifts (mock data)
pub async fn list_shifts() -> impl IntoResponse {
    let shifts = vec![Shift {
        id: "shift:1".to_string(),
        location: ShiftLocation {
            label: "Front desk".to_string(),
            floor: 1,
        },
        badge_label: "Front desk".to_string(),
        starts_on: "2024-01-01".to_string(),
        ends_on: "2024-01-08".to_string(),
        deposit: 40.0,
        balance: 60.0,
        first_name: "Ada".to_string(),
        last_name: "Lovelace".to_string(),
        full_name: "Ada Lovelace".to_string(),
        city: "New York".to_string(),
        state: "NY".to_string(),
        zip: "10001".to_string(),
        country: "USA".to_string(),
    }];

    Json(json!({
        "data": shifts,
        "count": shifts.len()
    }))
}
