use axum::{
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;

use crate::models::Role;

/// List all users (mock data)
pub async fn list_users() -> impl IntoResponse {
    let users = vec![
        mock_user("user:1", "alice@example.com", "alice"),
        mock_user("user:2", "bob@example.com", "bob"),
        mock_user("user:3", "charlie@example.com", "charlie"),
    ];

    Json(json!({
        "data": users,
        "count": users.len()
    }))
}

/// Get a single user by ID (mock data)
pub async fn get_user(Path(id): Path<String>) -> impl IntoResponse {
    if id == "1" || id == "user:1" {
        let user = mock_user("user:1", "alice@example.com", "alice");
        (StatusCode::OK, Json(json!({ "data": user })))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "User not found" })),
        )
    }
}

fn mock_user(id: &str, email: &str, username: &str) -> serde_json::Value {
    json!({
        "id": id,
        "email": email,
        "username": username,
        "roles": [Role::User],
        "is_active": true,
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
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
            .route("/users", get(list_users))
            .route("/users/{id}", get(get_user))
    }

    #[tokio::test]
    async fn test_list_users_returns_three_users() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/users").body(Body::empty()).unwrap())
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
    async fn test_list_users_contains_expected_users() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/users").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let users = json["data"].as_array().unwrap();
        let usernames: Vec<&str> = users
            .iter()
            .map(|u| u["username"].as_str().unwrap())
            .collect();

        assert!(usernames.contains(&"alice"));
        assert!(usernames.contains(&"bob"));
        assert!(usernames.contains(&"charlie"));
    }

    #[tokio::test]
    async fn test_get_user_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["data"]["username"], "alice");
        assert_eq!(json["data"]["email"], "alice@example.com");
    }

    #[tokio::test]
    async fn test_get_user_with_prefix_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users/user:1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["data"]["id"], "user:1");
    }

    #[tokio::test]
    async fn test_get_user_not_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users/999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json["error"].as_str().unwrap().contains("not found"));
    }

    #[test]
    fn test_mock_user_structure() {
        let user = mock_user("user:1", "test@example.com", "testuser");

        assert_eq!(user["id"], "user:1");
        assert_eq!(user["email"], "test@example.com");
        assert_eq!(user["username"], "testuser");
        assert_eq!(user["is_active"], true);
        assert!(user["roles"].is_array());
        assert!(user["created_at"].is_string());
        assert!(user["updated_at"].is_string());
    }
}
