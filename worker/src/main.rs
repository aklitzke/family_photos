use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use common::{
    ArtifactImages, ArtifactListResponse, ArtifactUpdate, ErrorResponse, HealthResponse,
    HistoryData, ImageListResponse, LoginRequest, LoginResponse, MeResponse,
    MergeArtifactsRequest, MergeArtifactsResponse, RotateImageRequest, RotateImageResponse,
    ThumbnailBatchRequest, ThumbnailBatchResponse, UpdateArtifactRequest, UpdateArtifactResponse,
};
use rand::Rng;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

const USERS: &[(&str, &str)] = &[
    ("testuser", "testpassword"),
    // Add more users here
];

#[derive(Clone)]
struct AppState {
    data_path: PathBuf,
    images_path: PathBuf,
    thumbs_path: PathBuf,
    /// Serialize writes to history.toml so concurrent rotations don't race.
    write_lock: std::sync::Arc<Mutex<()>>,
    /// In-memory session store: token -> username
    sessions: std::sync::Arc<Mutex<HashMap<String, String>>>,
}

/// Authenticated user extracted from session cookie.
struct AuthenticatedUser {
    username: String,
}

impl<S> axum::extract::FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let app_state = <AppState as axum::extract::FromRef<S>>::from_ref(state);
        let cookie_header = parts
            .headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = cookie_header
            .split(';')
            .filter_map(|s| {
                let s = s.trim();
                s.strip_prefix("session=")
            })
            .next();

        let token = match token {
            Some(t) if !t.is_empty() => t,
            _ => return Err((StatusCode::UNAUTHORIZED, "Not authenticated")),
        };

        let sessions: tokio::sync::MutexGuard<'_, HashMap<String, String>> =
            app_state.sessions.lock().await;
        match sessions.get(token) {
            Some(username) => Ok(AuthenticatedUser {
                username: username.clone(),
            }),
            None => Err((StatusCode::UNAUTHORIZED, "Invalid session")),
        }
    }
}

fn generate_session_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

fn session_cookie(token: &str, clear: bool) -> String {
    let max_age = if clear { 0 } else { 2592000 }; // 30 days
    let secure = if cfg!(debug_assertions) { "" } else { "; Secure" };
    format!(
        "session={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        token, max_age, secure
    )
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
        sessions: std::sync::Arc::new(Mutex::new(HashMap::new())),
    };

    // Serve frontend static files with index.html fallback for SPA routing
    let index_file = PathBuf::from(&frontend_path).join("index.html");
    let serve_frontend = ServeDir::new(&frontend_path)
        .not_found_service(ServeFile::new(&index_file));

    let cors = if cfg!(debug_assertions) {
        CorsLayer::new()
            .allow_origin("http://localhost:8080".parse::<axum::http::HeaderValue>().unwrap())
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any)
            .allow_credentials(true)
    } else {
        CorsLayer::permissive()
    };

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/images/list", get(images_list))
        .route("/api/artifacts/list", get(artifacts_list))
        .route("/api/images/thumbnail", get(thumbnail))
        .route("/api/images/thumbnails", post(thumbnails_batch))
        .route("/api/images/full", get(full_image))
        .route("/api/images/rotate", post(rotate))
        .route("/api/artifacts/update", post(update_artifact))
        .route("/api/artifacts/merge", post(merge_artifacts))
        .with_state(state)
        .fallback_service(serve_frontend)
        .layer(cors);

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

async fn login(
    State(state): State<AppState>,
    Json(request): Json<LoginRequest>,
) -> impl IntoResponse {
    let valid = USERS
        .iter()
        .any(|(u, p)| *u == request.username && *p == request.password);

    if !valid {
        return (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                success: false,
                username: String::new(),
            }),
        )
            .into_response();
    }

    let token = generate_session_token();
    {
        let mut sessions = state.sessions.lock().await;
        sessions.insert(token.clone(), request.username.clone());
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        session_cookie(&token, false).parse().unwrap(),
    );

    (
        StatusCode::OK,
        headers,
        Json(LoginResponse {
            success: true,
            username: request.username,
        }),
    )
        .into_response()
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Extract session token from cookie
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if let Some(token) = cookie_header
        .split(';')
        .filter_map(|s| s.trim().strip_prefix("session="))
        .next()
    {
        let mut sessions = state.sessions.lock().await;
        sessions.remove(token);
    }

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        header::SET_COOKIE,
        session_cookie("", true).parse().unwrap(),
    );

    (StatusCode::OK, resp_headers, Json(serde_json::json!({"success": true}))).into_response()
}

async fn me(user: AuthenticatedUser) -> Json<MeResponse> {
    Json(MeResponse {
        username: user.username,
    })
}

async fn images_list(
    _user: AuthenticatedUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
    match read_history(&state.data_path) {
        Ok(history) => Json(ImageListResponse { images: history.images }).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn artifacts_list(
    _user: AuthenticatedUser,
    State(state): State<AppState>,
) -> impl IntoResponse {
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
    _user: AuthenticatedUser,
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
    _user: AuthenticatedUser,
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
    _user: AuthenticatedUser,
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
    _user: AuthenticatedUser,
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

fn is_valid_date_format(s: &str) -> bool {
    // Match YYYY, YYYY-MM, or YYYY-MM-DD
    let bytes = s.as_bytes();
    if bytes.len() < 4 { return false; }
    if !bytes[0..4].iter().all(|b| b.is_ascii_digit()) { return false; }
    if bytes.len() == 4 { return true; }
    if bytes.len() < 7 || bytes[4] != b'-' { return false; }
    if !bytes[5..7].iter().all(|b| b.is_ascii_digit()) { return false; }
    let month: u8 = s[5..7].parse().unwrap_or(0);
    if !(1..=12).contains(&month) { return false; }
    if bytes.len() == 7 { return true; }
    if bytes.len() != 10 || bytes[7] != b'-' { return false; }
    if !bytes[8..10].iter().all(|b| b.is_ascii_digit()) { return false; }
    let day: u8 = s[8..10].parse().unwrap_or(0);
    (1..=31).contains(&day)
}

async fn update_artifact(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Json(request): Json<UpdateArtifactRequest>,
) -> impl IntoResponse {
    // Validate reason is non-empty
    if request.reason.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::to_value(ErrorResponse {
                error: "Reason is required".to_string(),
                error_type: "validation_error".to_string(),
            }).unwrap()),
        ).into_response();
    }

    // Validate date format if provided
    if let Some(ref date) = request.date {
        if !is_valid_date_format(date) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::to_value(ErrorResponse {
                    error: format!("Invalid date format: '{}'. Expected YYYY, YYYY-MM, or YYYY-MM-DD", date),
                    error_type: "validation_error".to_string(),
                }).unwrap()),
            ).into_response();
        }
    }

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

    let idx = match history.artifacts.iter().position(|a| a.id == request.artifact_id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(ErrorResponse {
                    error: format!("Artifact not found: {}", request.artifact_id),
                    error_type: "not_found".to_string(),
                }).unwrap()),
            ).into_response();
        }
    };

    let update = ArtifactUpdate {
        author: user.username.clone(),
        updated: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        reason: request.reason,
        date: request.date,
        tags: request.tags,
        people: request.people,
        location: request.location,
    };

    history.artifacts[idx].updates.push(update.clone());

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
        Json(serde_json::to_value(UpdateArtifactResponse {
            success: true,
            update,
        }).unwrap()),
    ).into_response()
}

async fn merge_artifacts(
    _user: AuthenticatedUser,
    State(state): State<AppState>,
    Json(request): Json<MergeArtifactsRequest>,
) -> impl IntoResponse {
    if request.leader_id == request.follower_id {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::to_value(ErrorResponse {
                error: "Cannot merge an artifact with itself".to_string(),
                error_type: "validation_error".to_string(),
            }).unwrap()),
        ).into_response();
    }

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

    let leader_idx = match history.artifacts.iter().position(|a| a.id == request.leader_id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(ErrorResponse {
                    error: format!("Leader artifact not found: {}", request.leader_id),
                    error_type: "not_found".to_string(),
                }).unwrap()),
            ).into_response();
        }
    };

    let follower_idx = match history.artifacts.iter().position(|a| a.id == request.follower_id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(ErrorResponse {
                    error: format!("Follower artifact not found: {}", request.follower_id),
                    error_type: "not_found".to_string(),
                }).unwrap()),
            ).into_response();
        }
    };

    // Clone follower data before mutating
    let follower = history.artifacts[follower_idx].clone();
    let leader = &history.artifacts[leader_idx];

    // Merge images: leader fronts + follower fronts, leader backs + follower backs
    let merged_images = ArtifactImages {
        fronts: leader.images.fronts.iter()
            .chain(follower.images.fronts.iter())
            .cloned()
            .collect(),
        backs: leader.images.backs.iter()
            .chain(follower.images.backs.iter())
            .cloned()
            .collect(),
    };

    // Merge updates: combine both, sort by timestamp, then add merge note
    let mut merged_updates: Vec<ArtifactUpdate> = leader.updates.iter()
        .chain(follower.updates.iter())
        .cloned()
        .collect();
    merged_updates.sort_by(|a, b| a.updated.cmp(&b.updated));

    merged_updates.push(ArtifactUpdate {
        author: "system".to_string(),
        updated: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        reason: format!("Merged artifact #{} into this artifact", request.follower_id),
        date: None,
        tags: None,
        people: None,
        location: None,
    });

    // Update leader in-place
    history.artifacts[leader_idx].images = merged_images;
    history.artifacts[leader_idx].updates = merged_updates;

    // Remove follower
    history.artifacts.retain(|a| a.id != request.follower_id);

    let merged_artifact = history.artifacts.iter()
        .find(|a| a.id == request.leader_id)
        .unwrap()
        .clone();

    // Write history
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
        Json(serde_json::to_value(MergeArtifactsResponse {
            success: true,
            merged_artifact,
        }).unwrap()),
    ).into_response()
}
