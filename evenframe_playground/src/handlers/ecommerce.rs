use axum::{
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;

use crate::models::{OrderStatus, ProductCategory};

/// List all products (mock data)
pub async fn list_products() -> impl IntoResponse {
    let products = vec![
        mock_product("product:1", "Laptop", ProductCategory::Electronics, 999.99),
        mock_product("product:2", "T-Shirt", ProductCategory::Clothing, 29.99),
        mock_product("product:3", "Rust Programming Book", ProductCategory::Books, 49.99),
    ];

    Json(json!({
        "data": products,
        "count": products.len()
    }))
}

/// Get a single product by ID (mock data)
pub async fn get_product(Path(id): Path<String>) -> impl IntoResponse {
    if id == "1" || id == "product:1" {
        let product = mock_product("product:1", "Laptop", ProductCategory::Electronics, 999.99);
        (StatusCode::OK, Json(json!({ "data": product })))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Product not found" })),
        )
    }
}

/// List all orders (mock data)
pub async fn list_orders() -> impl IntoResponse {
    let orders = vec![
        mock_order("order:1", "customer:1", 1029.98, OrderStatus::Delivered),
        mock_order("order:2", "customer:2", 79.98, OrderStatus::Processing),
        mock_order("order:3", "customer:1", 49.99, OrderStatus::Pending),
    ];

    Json(json!({
        "data": orders,
        "count": orders.len()
    }))
}

/// Get a single order by ID (mock data)
pub async fn get_order(Path(id): Path<String>) -> impl IntoResponse {
    if id == "1" || id == "order:1" {
        let order = mock_order("order:1", "customer:1", 1029.98, OrderStatus::Delivered);
        (StatusCode::OK, Json(json!({ "data": order })))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Order not found" })),
        )
    }
}

fn mock_product(id: &str, name: &str, category: ProductCategory, price: f64) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "description": format!("A great {} product", name.to_lowercase()),
        "price": price,
        "stock_quantity": 100,
        "category": category,
        "is_available": true,
        "created_at": "2024-01-01T00:00:00Z"
    })
}

fn mock_order(id: &str, customer_id: &str, total: f64, status: OrderStatus) -> serde_json::Value {
    json!({
        "id": id,
        "customer": customer_id,
        "items": [],
        "subtotal": total * 0.9,
        "tax": total * 0.1,
        "shipping_cost": 0.0,
        "total": total,
        "status": status,
        "shipping_address": {
            "street": "123 Main St",
            "city": "Springfield",
            "state": "IL",
            "postal_code": "62701",
            "country": "US"
        },
        "created_at": "2024-01-01T00:00:00Z"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/products", get(list_products))
            .route("/products/{id}", get(get_product))
            .route("/orders", get(list_orders))
            .route("/orders/{id}", get(get_order))
    }

    #[tokio::test]
    async fn test_list_products_returns_three_products() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/products").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["count"], 3);
        assert!(json["data"].is_array());
        assert_eq!(json["data"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_list_products_contains_expected_categories() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/products").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let products = json["data"].as_array().unwrap();
        let names: Vec<&str> = products
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();

        assert!(names.contains(&"Laptop"));
        assert!(names.contains(&"T-Shirt"));
        assert!(names.contains(&"Rust Programming Book"));
    }

    #[tokio::test]
    async fn test_get_product_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/products/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["data"]["name"], "Laptop");
        assert_eq!(json["data"]["price"], 999.99);
    }

    #[tokio::test]
    async fn test_get_product_not_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/products/999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_orders_returns_three_orders() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/orders").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["count"], 3);
    }

    #[tokio::test]
    async fn test_get_order_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/orders/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["data"]["id"], "order:1");
        assert_eq!(json["data"]["total"], 1029.98);
    }

    #[tokio::test]
    async fn test_get_order_not_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/orders/999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_mock_product_structure() {
        let product = mock_product("product:1", "Test", ProductCategory::Electronics, 99.99);

        assert_eq!(product["id"], "product:1");
        assert_eq!(product["name"], "Test");
        assert_eq!(product["price"], 99.99);
        assert_eq!(product["stock_quantity"], 100);
        assert_eq!(product["is_available"], true);
    }

    #[test]
    fn test_mock_order_structure() {
        let order = mock_order("order:1", "customer:1", 100.0, OrderStatus::Pending);

        assert_eq!(order["id"], "order:1");
        assert_eq!(order["customer"], "customer:1");
        assert_eq!(order["total"], 100.0);
        assert!(order["shipping_address"].is_object());
        assert_eq!(order["shipping_address"]["city"], "Springfield");
    }

    #[test]
    fn test_mock_order_tax_calculation() {
        let order = mock_order("order:1", "customer:1", 100.0, OrderStatus::Pending);

        let subtotal = order["subtotal"].as_f64().unwrap();
        let tax = order["tax"].as_f64().unwrap();
        let total = order["total"].as_f64().unwrap();

        // Tax should be 10% of total
        assert!((tax - 10.0).abs() < 0.01);
        // Subtotal should be 90% of total
        assert!((subtotal - 90.0).abs() < 0.01);
        // Total should match
        assert!((total - 100.0).abs() < 0.01);
    }
}
