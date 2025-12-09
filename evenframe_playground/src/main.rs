use axum::{
    Router,
    routing::get,
};
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod handlers;
mod models;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "evenframe_playground=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Build the router with all routes
    let app = Router::new()
        // Health check
        .route("/health", get(health_check))
        // Auth routes
        .route("/api/users", get(handlers::auth::list_users))
        .route("/api/users/:id", get(handlers::auth::get_user))
        // E-commerce routes
        .route("/api/products", get(handlers::ecommerce::list_products))
        .route("/api/products/:id", get(handlers::ecommerce::get_product))
        .route("/api/orders", get(handlers::ecommerce::list_orders))
        .route("/api/orders/:id", get(handlers::ecommerce::get_order))
        // Blog routes
        .route("/api/posts", get(handlers::blog::list_posts))
        .route("/api/posts/:id", get(handlers::blog::get_post))
        .route("/api/tags", get(handlers::blog::list_tags))
        .route("/api/posts/:post_id/comments", get(handlers::blog::list_comments));

    // Run the server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Evenframe Playground server listening on {}", addr);
    tracing::info!("Available endpoints:");
    tracing::info!("  GET /health");
    tracing::info!("  GET /api/users");
    tracing::info!("  GET /api/users/:id");
    tracing::info!("  GET /api/products");
    tracing::info!("  GET /api/products/:id");
    tracing::info!("  GET /api/orders");
    tracing::info!("  GET /api/orders/:id");
    tracing::info!("  GET /api/posts");
    tracing::info!("  GET /api/posts/:id");
    tracing::info!("  GET /api/tags");
    tracing::info!("  GET /api/posts/:post_id/comments");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "OK"
}
