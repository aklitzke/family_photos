use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use common::{
    ArtifactListResponse, HealthResponse, HistoryData, ImageListResponse,
    ThumbnailBatchRequest, ThumbnailBatchResponse,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    history: Arc<HistoryData>,
    thumbs_path: PathBuf,
}

#[tokio::main]
async fn main() {
    let data_path = std::env::var("DATA_PATH").unwrap_or_else(|_| "../data".to_string());
    let thumbs_path = std::env::var("THUMBS_PATH").unwrap_or_else(|_| "../images/thumbs".to_string());
    let frontend_path = std::env::var("FRONTEND_PATH").unwrap_or_else(|_| "../frontend/dist".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8082);

    // Load history.toml at startup
    let history_file = PathBuf::from(&data_path).join("history.toml");
    let history_content = std::fs::read_to_string(&history_file)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", history_file.display(), e));
    let history: HistoryData = toml::from_str(&history_content)
        .unwrap_or_else(|e| panic!("Failed to parse history.toml: {}", e));

    eprintln!("Loaded {} images, {} artifacts from {}", history.images.len(), history.artifacts.len(), history_file.display());

    let state = AppState {
        history: Arc::new(history),
        thumbs_path: PathBuf::from(&thumbs_path),
    };

    // Serve frontend static files with index.html fallback for SPA routing
    let index_file = PathBuf::from(&frontend_path).join("index.html");
    let serve_frontend = ServeDir::new(&frontend_path)
        .not_found_service(ServeFile::new(&index_file));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/images/list", get(images_list))
        .route("/api/artifacts/list", get(artifacts_list))
        .route("/api/images/thumbnail", get(thumbnail))
        .route("/api/images/thumbnails", post(thumbnails_batch))
        .route("/api/images/full", get(full_stub))
        .route("/api/images/rotate", post(rotate_stub))
        .with_state(state)
        .fallback_service(serve_frontend)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap();
    eprintln!("Listening on http://0.0.0.0:{}", port);
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        message: "Server is running".to_string(),
    })
}

async fn images_list(State(state): State<AppState>) -> Json<ImageListResponse> {
    Json(ImageListResponse {
        images: state.history.images.clone(),
    })
}

async fn artifacts_list(State(state): State<AppState>) -> Json<ArtifactListResponse> {
    Json(ArtifactListResponse {
        artifacts: state.history.artifacts.clone(),
    })
}

#[derive(serde::Deserialize)]
struct KeyParam {
    key: String,
}

/// Resolve an image key to the thumbnail file path on disk.
/// Thumbnail files are always .jpg, stored directly under thumbs_path
/// (no bucket prefix — the key is the relative path).
fn thumb_file(thumbs_path: &Path, key: &str) -> PathBuf {
    let p = Path::new(key);
    let jpg_name = p.with_extension("jpg");
    thumbs_path.join(jpg_name)
}

async fn thumbnail(
    State(state): State<AppState>,
    Query(params): Query<KeyParam>,
) -> impl IntoResponse {
    let path = thumb_file(&state.thumbs_path, &params.key);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                ("content-type", "image/jpeg"),
                ("cache-control", "public, max-age=31536000"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "Thumbnail not found").into_response(),
    }
}

async fn thumbnails_batch(
    State(state): State<AppState>,
    Json(request): Json<ThumbnailBatchRequest>,
) -> Json<ThumbnailBatchResponse> {
    let mut thumbnails: HashMap<String, Vec<u8>> = HashMap::new();
    for key in &request.keys {
        let path = thumb_file(&state.thumbs_path, key);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            thumbnails.insert(key.clone(), bytes);
        }
    }
    Json(ThumbnailBatchResponse { thumbnails })
}

async fn full_stub() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Full image serving not yet implemented")
}

async fn rotate_stub() -> impl IntoResponse {
    (StatusCode::NOT_IMPLEMENTED, "Rotation not yet implemented")
}
