use axum::{
    routing::get,
    Router,
    response::Json,
};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use std::net::SocketAddr;
use std::path::Path;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Find the frontend dist directory - try multiple possible paths
    let frontend_path = find_frontend_dist().expect("Could not find frontend/dist directory. Run 'npm run build' in the frontend directory.");

    tracing::info!("Serving frontend from: {}", frontend_path);

    // Build our application with routes
    let app = Router::new()
        // API health check endpoint
        .route("/api/health", get(health_check))
        // Serve static files from the frontend build directory as fallback
        .fallback_service(ServeDir::new(frontend_path))
        // Add tracing
        .layer(TraceLayer::new_for_http());

    // Run the server
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Server listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn find_frontend_dist() -> Option<String> {
    // Possible paths to check, in order of preference
    let paths = vec![
        "../frontend/dist",      // Running from backend/
        "frontend/dist",         // Running from project root
        "./frontend/dist",       // Alternative project root
        "../../frontend/dist",   // Running from backend/target/debug or similar
    ];

    for path in paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    None
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "message": "Server is running"
    }))
}
