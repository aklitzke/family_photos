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
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct AppState {
    data_path: PathBuf,
    images_path: PathBuf,
    thumbs_path: PathBuf,
    /// Serialize writes to history.toml so concurrent rotations don't race.
    write_lock: std::sync::Arc<Mutex<()>>,
}

fn read_history(data_path: &Path) -> Result<HistoryData, String> {
    let history_file = data_path.join("history.toml");
    let content = std::fs::read_to_string(&history_file)
        .map_err(|e| format!("Failed to read {}: {}", history_file.display(), e))?;
    toml::from_str(&content)
        .map_err(|e| format!("Failed to parse history.toml: {}", e))
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

    // Validate history.toml is readable at startup
    let data_path = PathBuf::from(&data_path);
    let history = read_history(&data_path)
        .unwrap_or_else(|e| panic!("{}", e));
    eprintln!("Loaded {} images, {} artifacts from {}",
        history.images.len(), history.artifacts.len(), data_path.join("history.toml").display());

    let state = AppState {
        data_path,
        images_path: PathBuf::from(&images_path),
        thumbs_path: PathBuf::from(&thumbs_path),
        write_lock: std::sync::Arc::new(Mutex::new(())),
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

async fn images_list(State(state): State<AppState>) -> impl IntoResponse {
    match read_history(&state.data_path) {
        Ok(history) => Json(ImageListResponse { images: history.images }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn artifacts_list(State(state): State<AppState>) -> impl IntoResponse {
    match read_history(&state.data_path) {
        Ok(history) => Json(ArtifactListResponse { artifacts: history.artifacts }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
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
    let _lock = state.write_lock.lock().await;

    let mut history = match read_history(&state.data_path) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::to_value(ErrorResponse {
                    error: e,
                    error_type: "read_error".to_string(),
                }).unwrap()),
            ).into_response();
        }
    };

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
    history.images[idx].rotation = if request.new_rotation == 0 {
        None
    } else {
        Some(request.new_rotation)
    };

    // Write updated history to disk
    let history_file = state.data_path.join("history.toml");
    match common::format_history_toml(&history) {
        Ok(toml_str) => {
            if let Err(e) = tokio::fs::write(&history_file, &toml_str).await {
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
