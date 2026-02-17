use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use common::{
    ArtifactListResponse, ErrorResponse, HealthResponse, HistoryData, ImageListResponse,
    RotateImageRequest, RotateImageResponse, ThumbnailBatchRequest, ThumbnailBatchResponse,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    history: Arc<RwLock<HistoryData>>,
    data_path: PathBuf,
    images_path: PathBuf,
    thumbs_path: PathBuf,
}

#[tokio::main]
async fn main() {
    let data_path = std::env::var("DATA_PATH").unwrap_or_else(|_| "../data".to_string());
    let images_path = std::env::var("IMAGES_PATH").unwrap_or_else(|_| "../images/lossless".to_string());
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
        history: Arc::new(RwLock::new(history)),
        data_path: PathBuf::from(&data_path),
        images_path: PathBuf::from(&images_path),
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
        .route("/api/images/full", get(full_image))
        .route("/api/images/rotate", post(rotate))
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
    let history = state.history.read().await;
    Json(ImageListResponse {
        images: history.images.clone(),
    })
}

async fn artifacts_list(State(state): State<AppState>) -> Json<ArtifactListResponse> {
    let history = state.history.read().await;
    Json(ArtifactListResponse {
        artifacts: history.artifacts.clone(),
    })
}

#[derive(serde::Deserialize)]
struct KeyParam {
    key: String,
}

/// Find the actual file for an extensionless key by scanning the parent directory.
async fn resolve_key(base: &Path, key: &str) -> Option<PathBuf> {
    let key_path = base.join(key);
    let dir = key_path.parent()?;
    let stem = key_path.file_name()?.to_str()?;
    let mut entries = tokio::fs::read_dir(dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name = name.to_str()?;
        if let Some(base_name) = name.rsplit_once('.').map(|(b, _)| b) {
            if base_name == stem {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Thumbnail files are always .jpg.
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

async fn full_image(
    State(state): State<AppState>,
    Query(params): Query<KeyParam>,
) -> impl IntoResponse {
    match resolve_key(&state.images_path, &params.key).await {
        Some(path) => match tokio::fs::read(&path).await {
            Ok(bytes) => (
                StatusCode::OK,
                [("content-type", "application/octet-stream")],
                bytes,
            ).into_response(),
            Err(_) => (StatusCode::NOT_FOUND, "Image not found").into_response(),
        },
        None => (StatusCode::NOT_FOUND, "Image not found").into_response(),
    }
}

async fn rotate(
    State(state): State<AppState>,
    Json(request): Json<RotateImageRequest>,
) -> impl IntoResponse {
    let mut history = state.history.write().await;

    let idx = match history.images.iter().position(|img| img.key == request.image_key) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(ErrorResponse {
                    error: format!("Image not found: {}", request.image_key),
                    error_type: "not_found".to_string(),
                }).unwrap()),
            ).into_response();
        }
    };

    let old_rotation = history.images[idx].rotation;
    let new_rotation = if request.new_rotation == 0 {
        None
    } else {
        Some(request.new_rotation)
    };
    history.images[idx].rotation = new_rotation;

    // Write updated history to disk
    let history_file = state.data_path.join("history.toml");
    match common::format_history_toml(&history) {
        Ok(toml_str) => {
            if let Err(e) = tokio::fs::write(&history_file, &toml_str).await {
                history.images[idx].rotation = old_rotation;
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::to_value(ErrorResponse {
                        error: format!("Failed to write history.toml: {}", e),
                        error_type: "io_error".to_string(),
                    }).unwrap()),
                ).into_response();
            }
        }
        Err(e) => {
            history.images[idx].rotation = old_rotation;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::to_value(ErrorResponse {
                    error: format!("Failed to format history.toml: {}", e),
                    error_type: "format_error".to_string(),
                }).unwrap()),
            ).into_response();
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::to_value(RotateImageResponse {
            success: true,
            old_rotation,
            new_rotation: request.new_rotation,
        }).unwrap()),
    ).into_response()
}
