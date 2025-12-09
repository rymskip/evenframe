use axum::{
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;

/// List all posts (mock data)
pub async fn list_posts() -> impl IntoResponse {
    let posts = vec![
        mock_post("post:1", "Getting Started with Rust", "getting-started-with-rust", true),
        mock_post("post:2", "Building Web APIs with Axum", "building-web-apis-with-axum", true),
        mock_post("post:3", "Draft Post", "draft-post", false),
    ];

    // Filter to only published posts
    let published: Vec<_> = posts.into_iter().filter(|p| p["published"] == true).collect();

    Json(json!({
        "data": published,
        "count": published.len()
    }))
}

/// Get a single post by ID or slug (mock data)
pub async fn get_post(Path(id): Path<String>) -> impl IntoResponse {
    if id == "1" || id == "post:1" || id == "getting-started-with-rust" {
        let post = mock_post("post:1", "Getting Started with Rust", "getting-started-with-rust", true);
        (StatusCode::OK, Json(json!({ "data": post })))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Post not found" })),
        )
    }
}

/// List all tags (mock data)
pub async fn list_tags() -> impl IntoResponse {
    let tags = vec![
        mock_tag("tag:1", "Rust", "rust"),
        mock_tag("tag:2", "Web Development", "web-development"),
        mock_tag("tag:3", "Tutorial", "tutorial"),
    ];

    Json(json!({
        "data": tags,
        "count": tags.len()
    }))
}

/// List comments for a post (mock data)
pub async fn list_comments(Path(post_id): Path<String>) -> impl IntoResponse {
    let comments = vec![
        mock_comment("comment:1", &post_id, "author:1", "Great article!"),
        mock_comment("comment:2", &post_id, "author:2", "Very helpful, thanks!"),
    ];

    Json(json!({
        "data": comments,
        "count": comments.len()
    }))
}

fn mock_post(id: &str, title: &str, slug: &str, published: bool) -> serde_json::Value {
    json!({
        "id": id,
        "title": title,
        "slug": slug,
        "content": format!("This is the content for {}...", title),
        "excerpt": format!("A brief excerpt about {}...", title.to_lowercase()),
        "author": "author:1",
        "tags": ["tag:1", "tag:3"],
        "published": published,
        "published_at": if published { Some("2024-01-01T00:00:00Z") } else { None },
        "view_count": 100,
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
    })
}

fn mock_tag(id: &str, name: &str, slug: &str) -> serde_json::Value {
    json!({
        "id": id,
        "name": name,
        "slug": slug,
        "post_count": 5
    })
}

fn mock_comment(id: &str, post_id: &str, author_id: &str, content: &str) -> serde_json::Value {
    json!({
        "id": id,
        "post": post_id,
        "author": author_id,
        "content": content,
        "is_approved": true,
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
            .route("/posts", get(list_posts))
            .route("/posts/{id}", get(get_post))
            .route("/posts/{post_id}/comments", get(list_comments))
            .route("/tags", get(list_tags))
    }

    #[tokio::test]
    async fn test_list_posts_returns_only_published() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/posts").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Should only return 2 published posts, not the draft
        assert_eq!(json["count"], 2);
        assert!(json["data"].is_array());
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_list_posts_all_are_published() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/posts").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let posts = json["data"].as_array().unwrap();
        for post in posts {
            assert_eq!(post["published"], true, "All returned posts should be published");
        }
    }

    #[tokio::test]
    async fn test_get_post_by_id() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/posts/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["data"]["title"], "Getting Started with Rust");
    }

    #[tokio::test]
    async fn test_get_post_by_prefixed_id() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/posts/post:1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["data"]["id"], "post:1");
    }

    #[tokio::test]
    async fn test_get_post_by_slug() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/posts/getting-started-with-rust")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["data"]["slug"], "getting-started-with-rust");
    }

    #[tokio::test]
    async fn test_get_post_not_found() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/posts/nonexistent")
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

    #[tokio::test]
    async fn test_list_tags_returns_three_tags() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/tags").body(Body::empty()).unwrap())
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
    async fn test_list_tags_contains_expected_tags() {
        let app = app();

        let response = app
            .oneshot(Request::builder().uri("/tags").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let tags = json["data"].as_array().unwrap();
        let names: Vec<&str> = tags.iter().map(|t| t["name"].as_str().unwrap()).collect();

        assert!(names.contains(&"Rust"));
        assert!(names.contains(&"Web Development"));
        assert!(names.contains(&"Tutorial"));
    }

    #[tokio::test]
    async fn test_list_comments_for_post() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/posts/1/comments")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["count"], 2);
        assert!(json["data"].is_array());
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_list_comments_includes_post_reference() {
        let app = app();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/posts/post:1/comments")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let comments = json["data"].as_array().unwrap();
        for comment in comments {
            assert_eq!(
                comment["post"], "post:1",
                "Each comment should reference the post"
            );
        }
    }

    #[test]
    fn test_mock_post_structure() {
        let post = mock_post("post:1", "Test Title", "test-title", true);

        assert_eq!(post["id"], "post:1");
        assert_eq!(post["title"], "Test Title");
        assert_eq!(post["slug"], "test-title");
        assert_eq!(post["published"], true);
        assert!(post["content"].is_string());
        assert!(post["excerpt"].is_string());
        assert!(post["tags"].is_array());
        assert_eq!(post["view_count"], 100);
    }

    #[test]
    fn test_mock_post_unpublished_has_no_published_at() {
        let post = mock_post("post:1", "Draft", "draft", false);

        assert_eq!(post["published"], false);
        assert!(post["published_at"].is_null());
    }

    #[test]
    fn test_mock_tag_structure() {
        let tag = mock_tag("tag:1", "Test Tag", "test-tag");

        assert_eq!(tag["id"], "tag:1");
        assert_eq!(tag["name"], "Test Tag");
        assert_eq!(tag["slug"], "test-tag");
        assert_eq!(tag["post_count"], 5);
    }

    #[test]
    fn test_mock_comment_structure() {
        let comment = mock_comment("comment:1", "post:1", "author:1", "Test content");

        assert_eq!(comment["id"], "comment:1");
        assert_eq!(comment["post"], "post:1");
        assert_eq!(comment["author"], "author:1");
        assert_eq!(comment["content"], "Test content");
        assert_eq!(comment["is_approved"], true);
        assert!(comment["created_at"].is_string());
    }
}
