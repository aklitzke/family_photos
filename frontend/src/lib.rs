use common::{artifact_date, Artifact, ArtifactListResponse, ArtifactUpdate, ErrorResponse, HealthResponse, ImageListResponse, ImageMetadata, RotateImageRequest, RotateImageResponse, ThumbnailBatchRequest, ThumbnailBatchResponse, UpdateArtifactRequest, UpdateArtifactResponse};
use eframe::egui::{self, ColorImage, TextureHandle};
use eframe::epaint::Vec2;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

const API_BASE_URL: &str = if cfg!(debug_assertions) {
    "http://localhost:8082"
} else {
    ""
};

// UI Constants
const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 50.0;
const ZOOM_DEFAULT: f32 = 1.0;
const THUMBNAIL_BATCH_SIZE: usize = 10;
const MAX_TEXTURE_SIDE: u32 = 8192;

#[derive(Clone)]
struct FullImageLoaded {
    color_image: ColorImage,
    raw_bytes: Vec<u8>,
}

#[derive(Clone, PartialEq)]
enum Page {
    Images,
    Artifacts,
    Health,
}

#[derive(Clone)]
enum LoadState<T: Clone> {
    NotStarted,
    Loading,
    Loaded(T),
    Failed(String),
}

// Generic async resource wrapper to eliminate duplicated loading pattern
struct AsyncResource<T: Clone> {
    state: LoadState<T>,
    loading: Option<Arc<Mutex<LoadState<T>>>>,
}

impl<T: Clone> AsyncResource<T> {
    fn new() -> Self {
        Self {
            state: LoadState::NotStarted,
            loading: None,
        }
    }

    fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    fn is_not_started(&self) -> bool {
        matches!(self.state, LoadState::NotStarted)
    }

    fn start_loading(&mut self) -> Arc<Mutex<LoadState<T>>> {
        self.state = LoadState::Loading;
        let loading_state = Arc::new(Mutex::new(LoadState::<T>::Loading));
        self.loading = Some(loading_state.clone());
        loading_state
    }

    fn process(&mut self) {
        let should_update = if let Some(loading_state) = &self.loading {
            let state = loading_state.lock().unwrap();
            match &*state {
                LoadState::Loaded(data) => Some(LoadState::Loaded(data.clone())),
                LoadState::Failed(err) => Some(LoadState::Failed(err.clone())),
                _ => None,
            }
        } else {
            None
        };

        if let Some(new_state) = should_update {
            self.state = new_state;
            self.loading = None;
        }
    }

    fn get(&self) -> &LoadState<T> {
        &self.state
    }
}

// Zoom controller for image viewing
struct ZoomController {
    zoom: f32,
    scroll_offset: Vec2,
}

impl ZoomController {
    fn new() -> Self {
        Self {
            zoom: ZOOM_DEFAULT,
            scroll_offset: Vec2::ZERO,
        }
    }

    fn reset(&mut self) {
        self.zoom = ZOOM_DEFAULT;
        self.scroll_offset = Vec2::ZERO;
    }

    fn apply_zoom_delta(&mut self, delta: f32) {
        self.zoom = (self.zoom * delta).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Calculate new scroll offset to zoom towards a specific point
    fn calculate_zoom_to_cursor(
        &self,
        old_zoom: f32,
        new_zoom: f32,
        mouse_in_viewport: Vec2,
        texture_size: Vec2,
        base_scale: f32,
        available_size: Vec2,
    ) -> Vec2 {
        // Calculate old padding and display size
        let old_display_size = texture_size * (base_scale * old_zoom);
        let old_x_padding = ((available_size.x - old_display_size.x) / 2.0).max(0.0);
        let old_y_padding = ((available_size.y - old_display_size.y) / 2.0).max(0.0);

        // New padding and display size
        let new_display_size = texture_size * (base_scale * new_zoom);
        let new_x_padding = ((available_size.x - new_display_size.x) / 2.0).max(0.0);
        let new_y_padding = ((available_size.y - new_display_size.y) / 2.0).max(0.0);

        // Position in old image (accounting for scroll and padding)
        let point_in_old_image_x = self.scroll_offset.x + mouse_in_viewport.x - old_x_padding;
        let point_in_old_image_y = self.scroll_offset.y + mouse_in_viewport.y - old_y_padding;

        // Normalized position (0-1) in the actual image
        let norm_x = (point_in_old_image_x / old_display_size.x).clamp(0.0, 1.0);
        let norm_y = (point_in_old_image_y / old_display_size.y).clamp(0.0, 1.0);

        // Where that point is in the new zoomed image
        let point_in_new_image_x = norm_x * new_display_size.x;
        let point_in_new_image_y = norm_y * new_display_size.y;

        // Calculate new scroll offset to keep that point under mouse
        let new_offset_x = (point_in_new_image_x + new_x_padding - mouse_in_viewport.x).max(0.0);
        let new_offset_y = (point_in_new_image_y + new_y_padding - mouse_in_viewport.y).max(0.0);

        egui::vec2(new_offset_x, new_offset_y)
    }
}

#[wasm_bindgen]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no global `window` exists")
            .document()
            .expect("should have a document on window");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("canvas element not found")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("element is not a canvas");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(FamilyPhotosApp::new(cc)))),
            )
            .await;

        match start_result {
            Ok(_) => {}
            Err(e) => {
                web_sys::console::error_1(&format!("Failed to start eframe: {e:?}").into());
            }
        }
    });

    Ok(())
}

struct FamilyPhotosApp {
    images: AsyncResource<Vec<ImageMetadata>>,
    artifacts: AsyncResource<Vec<Artifact>>,
    thumbnails: HashMap<String, TextureHandle>,
    thumbnail_loading: HashMap<String, Arc<Mutex<LoadState<Vec<u8>>>>>,
    thumbnail_failures: HashMap<String, String>,  // Track permanent failures
    full_images: HashMap<String, TextureHandle>,
    full_images_bytes: HashMap<String, Vec<u8>>,  // Cache raw bytes for rotation updates
    full_images_loading: HashMap<String, Arc<Mutex<LoadState<FullImageLoaded>>>>,
    full_image_failures: HashMap<String, String>,  // Track permanent failures
    zoom_controller: ZoomController,
    health: AsyncResource<HealthResponse>,
    rotation_updating: HashMap<String, bool>,  // Track images being rotated
    rotation_promises: HashMap<String, Arc<Mutex<LoadState<RotateImageResponse>>>>,  // Track rotation update promises
    toast_message: Option<(String, bool)>,     // (message, is_error)
    toast_timer: f64,                          // Timer for toast auto-dismiss
    search_query: String,
    images_visible_range: (usize, usize),      // (first, last) visible row indices
    artifacts_visible_range: (usize, usize),   // (first, last) visible row indices
    update_date_state: HashMap<u32, String>,      // In-progress date edits per artifact
    update_reason_state: HashMap<u32, String>,   // In-progress reason edits per artifact
    update_in_progress: HashMap<u32, bool>,      // Track artifacts being updated
    update_promises: HashMap<u32, Arc<Mutex<LoadState<UpdateArtifactResponse>>>>,
    update_validation_error: HashMap<u32, String>, // Inline validation feedback
}

impl FamilyPhotosApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Redirect root URL to /artifacts
        if let Some(window) = web_sys::window() {
            if let Ok(location) = window.location().pathname() {
                if location == "/" {
                    let history = window.history().unwrap();
                    let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some("/artifacts"));
                }
            }
        }

        Self {
            images: AsyncResource::new(),
            artifacts: AsyncResource::new(),
            thumbnails: HashMap::new(),
            thumbnail_loading: HashMap::new(),
            thumbnail_failures: HashMap::new(),
            full_images: HashMap::new(),
            full_images_bytes: HashMap::new(),
            full_images_loading: HashMap::new(),
            full_image_failures: HashMap::new(),
            zoom_controller: ZoomController::new(),
            health: AsyncResource::new(),
            rotation_updating: HashMap::new(),
            rotation_promises: HashMap::new(),
            toast_message: None,
            toast_timer: 0.0,
            search_query: String::new(),
            images_visible_range: (0, 0),
            artifacts_visible_range: (0, 0),
            update_date_state: HashMap::new(),
            update_reason_state: HashMap::new(),
            update_in_progress: HashMap::new(),
            update_promises: HashMap::new(),
            update_validation_error: HashMap::new(),
        }
    }

    /// Get selected image key from URL (if on /images?key=...)
    fn get_selected_image(&self) -> Option<String> {
        let window = web_sys::window()?;
        let location = window.location();
        let pathname = location.pathname().ok()?;

        if pathname.starts_with("/images") {
            if let Ok(search) = location.search() {
                return Self::parse_query_param(&search, "key");
            }
        }
        None
    }

    /// Get selected artifact ID from URL (if on /artifacts/{id})
    fn get_selected_artifact(&self) -> Option<u32> {
        let window = web_sys::window()?;
        let location = window.location();
        let pathname = location.pathname().ok()?;

        let parts: Vec<&str> = pathname.trim_matches('/').split('/').collect();
        if parts.get(0)? == &"artifacts" {
            if let Some(id_str) = parts.get(1) {
                return id_str.parse().ok();
            }
        }
        None
    }

    /// Get the current page from the URL
    fn get_current_page(&self) -> Page {
        if let Some(window) = web_sys::window() {
            if let Ok(location) = window.location().pathname() {
                let parts: Vec<&str> = location.trim_matches('/').split('/').collect();
                return match parts.get(0) {
                    Some(&"images") => Page::Images,
                    Some(&"health") => Page::Health,
                    _ => Page::Artifacts,
                };
            }
        }
        Page::Artifacts // Default fallback
    }

    fn parse_query_param(query: &str, param: &str) -> Option<String> {
        for part in query.trim_start_matches('?').split('&') {
            if let Some((key, value)) = part.split_once('=') {
                if key == param {
                    return urlencoding::decode(value).ok().map(|s| s.into_owned());
                }
            }
        }
        None
    }

    /// Navigate to a specific image
    fn navigate_to_image(&self, key: &str) {
        let window = web_sys::window().unwrap();
        let history = window.history().unwrap();
        let path = format!("/images?key={}", urlencoding::encode(key));
        let _ = history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
    }

    /// Navigate to a specific artifact by ID
    fn navigate_to_artifact(&self, id: u32) {
        let window = web_sys::window().unwrap();
        let history = window.history().unwrap();
        let path = format!("/artifacts/{}", id);
        let _ = history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
    }

    /// Navigate to a page without selection
    fn navigate_to_page(&self, page: Page) {
        let window = web_sys::window().unwrap();
        let history = window.history().unwrap();
        let path = match page {
            Page::Images => "/images".to_string(),
            Page::Artifacts => "/artifacts".to_string(),
            Page::Health => "/health".to_string(),
        };
        let _ = history.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
    }

    fn load_image_list(&mut self, ctx: &egui::Context) {
        if self.images.is_loading() || !self.images.is_not_started() {
            return;
        }

        let images_state = self.images.start_loading();
        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_json::<ImageListResponse>("/api/images/list").await {
                Ok(response) => {
                    *images_state.lock().unwrap() = LoadState::Loaded(response.images);
                    ctx_clone.request_repaint();
                }
                Err(e) => {
                    *images_state.lock().unwrap() = LoadState::Failed(e);
                    ctx_clone.request_repaint();
                }
            }
        });
    }

    fn load_thumbnails_batch(&mut self, keys: Vec<String>, ctx: &egui::Context) {
        // Filter out keys that are already loaded, loading, or permanently failed
        let keys_to_load: Vec<String> = keys
            .into_iter()
            .filter(|key| {
                !self.thumbnails.contains_key(key)
                    && !self.thumbnail_loading.contains_key(key)
                    && !self.thumbnail_failures.contains_key(key)
            })
            .collect();

        if keys_to_load.is_empty() {
            return;
        }

        // Mark all as loading
        let loading_states: HashMap<String, Arc<Mutex<LoadState<Vec<u8>>>>> = keys_to_load
            .iter()
            .map(|key| {
                let state = Arc::new(Mutex::new(LoadState::Loading));
                self.thumbnail_loading.insert(key.clone(), state.clone());
                (key.clone(), state)
            })
            .collect();

        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_thumbnails_batch(&keys_to_load).await {
                Ok(thumbnails) => {
                    // Update each thumbnail's state
                    for (key, image_data) in thumbnails {
                        if let Some(state) = loading_states.get(&key) {
                            *state.lock().unwrap() = LoadState::Loaded(image_data);
                        }
                    }
                    ctx_clone.request_repaint();
                }
                Err(e) => {
                    // Mark all as failed
                    for state in loading_states.values() {
                        *state.lock().unwrap() = LoadState::Failed(e.clone());
                    }
                    ctx_clone.request_repaint();
                }
            }
        });
    }

    fn load_full_image(&mut self, key: &str, ctx: &egui::Context) {
        // Don't retry if already loaded, loading, or permanently failed
        if self.full_images.contains_key(key)
            || self.full_images_loading.contains_key(key)
            || self.full_image_failures.contains_key(key) {
            return;
        }

        // Need rotation metadata from images list
        let rotation = if let LoadState::Loaded(images) = self.images.get() {
            images.iter()
                .find(|img| img.key == key)
                .and_then(|img| img.rotation)
        } else {
            return; // Wait for images list to load
        };

        let loading_state = Arc::new(Mutex::new(LoadState::Loading));
        self.full_images_loading.insert(key.to_string(), loading_state.clone());
        let ctx_clone = ctx.clone();

        // Use cached raw bytes if available (for rotation re-renders)
        if let Some(bytes) = self.full_images_bytes.get(key) {
            let bytes = bytes.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match decode_full_image(&bytes, rotation).await {
                    Ok(color_image) => {
                        *loading_state.lock().unwrap() = LoadState::Loaded(FullImageLoaded {
                            color_image,
                            raw_bytes: bytes,
                        });
                        ctx_clone.request_repaint();
                    }
                    Err(e) => {
                        *loading_state.lock().unwrap() = LoadState::Failed(e);
                        ctx_clone.request_repaint();
                    }
                }
            });
            return;
        }

        // Fetch from server, then decode using browser-native APIs
        let key_encoded = urlencoding::encode(key).to_string();
        wasm_bindgen_futures::spawn_local(async move {
            match fetch_image_from_url(&format!("{}/api/images/full?key={}", API_BASE_URL, key_encoded)).await {
                Ok(bytes) => {
                    match decode_full_image(&bytes, rotation).await {
                        Ok(color_image) => {
                            *loading_state.lock().unwrap() = LoadState::Loaded(FullImageLoaded {
                                color_image,
                                raw_bytes: bytes,
                            });
                            ctx_clone.request_repaint();
                        }
                        Err(e) => {
                            *loading_state.lock().unwrap() = LoadState::Failed(e);
                            ctx_clone.request_repaint();
                        }
                    }
                }
                Err(e) => {
                    *loading_state.lock().unwrap() = LoadState::Failed(format!("Failed to fetch image: {}", e));
                    ctx_clone.request_repaint();
                }
            }
        });
    }

    fn process_loaded_thumbnails(&mut self, ctx: &egui::Context) {
        let mut completed = Vec::new();

        // Get rotation info from loaded images
        let rotation_map: HashMap<String, Option<u16>> = if let LoadState::Loaded(images) = self.images.get() {
            images.iter().map(|img| (img.key.clone(), img.rotation)).collect()
        } else {
            HashMap::new()
        };

        for (id, loading_state) in &self.thumbnail_loading {
            let state = loading_state.lock().unwrap();
            match &*state {
                LoadState::Loaded(data) => {
                    let rotation = rotation_map.get(id).and_then(|r| *r);
                    if let Some(color_image) = load_image_from_bytes(data, rotation) {
                        let texture = ctx.load_texture(
                            format!("thumbnail_{}", id),
                            color_image,
                            Default::default(),
                        );
                        self.thumbnails.insert(id.clone(), texture);
                    }
                    completed.push((id.clone(), None));
                }
                LoadState::Failed(err) => {
                    // Store permanent failure
                    completed.push((id.clone(), Some(err.clone())));
                }
                _ => {}
            }
        }

        for (id, error) in completed {
            self.thumbnail_loading.remove(&id);
            if let Some(err) = error {
                self.thumbnail_failures.insert(id, err);
            }
        }
    }

    fn process_loaded_full_images(&mut self, ctx: &egui::Context) {
        let mut completed = Vec::new();

        for (id, loading_state) in &self.full_images_loading {
            let state = loading_state.lock().unwrap();
            match &*state {
                LoadState::Loaded(data) => {
                    // Cache raw bytes for rotation re-renders
                    self.full_images_bytes.insert(id.clone(), data.raw_bytes.clone());

                    let texture = ctx.load_texture(
                        format!("full_image_{}", id),
                        data.color_image.clone(),
                        Default::default(),
                    );
                    self.full_images.insert(id.clone(), texture);
                    completed.push((id.clone(), None));
                }
                LoadState::Failed(err) => {
                    completed.push((id.clone(), Some(err.clone())));
                }
                _ => {}
            }
        }

        for (id, error) in completed {
            self.full_images_loading.remove(&id);
            if let Some(err) = error {
                self.full_image_failures.insert(id, err);
            }
        }
    }

    fn load_artifacts(&mut self, ctx: &egui::Context) {
        if self.artifacts.is_loading() || !self.artifacts.is_not_started() {
            return;
        }

        let artifacts_state = self.artifacts.start_loading();
        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_json::<ArtifactListResponse>("/api/artifacts/list").await {
                Ok(response) => {
                    *artifacts_state.lock().unwrap() = LoadState::Loaded(response.artifacts);
                    ctx_clone.request_repaint();
                }
                Err(e) => {
                    *artifacts_state.lock().unwrap() = LoadState::Failed(e);
                    ctx_clone.request_repaint();
                }
            }
        });
    }

    fn load_health(&mut self, ctx: &egui::Context) {
        if self.health.is_loading() || !self.health.is_not_started() {
            return;
        }

        let health_state = self.health.start_loading();
        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_json::<HealthResponse>("/api/health").await {
                Ok(response) => {
                    *health_state.lock().unwrap() = LoadState::Loaded(response);
                    ctx_clone.request_repaint();
                }
                Err(e) => {
                    *health_state.lock().unwrap() = LoadState::Failed(e);
                    ctx_clone.request_repaint();
                }
            }
        });
    }

    fn close_image_view(&mut self) {
        self.zoom_controller.reset();
        // Use browser back navigation to return to previous page
        // If no history, fallback to images page
        if let Some(window) = web_sys::window() {
            if let Ok(history) = window.history() {
                // Check if there's history to go back to
                if let Ok(length) = history.length() {
                    if length > 1 {
                        let _ = history.back();
                    } else {
                        // No history, navigate to images page
                        self.navigate_to_page(Page::Images);
                    }
                } else {
                    // Can't determine length, try back anyway
                    let _ = history.back();
                }
            }
        }
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(150.0)
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.heading("Family Photos");
                ui.add_space(20.0);

                if ui.selectable_label(self.get_current_page() == Page::Artifacts, "Artifacts").clicked() {
                    self.navigate_to_page(Page::Artifacts);
                }

                if ui.selectable_label(self.get_current_page() == Page::Images, "Images").clicked() {
                    self.navigate_to_page(Page::Images);
                }

                if ui.selectable_label(self.get_current_page() == Page::Health, "Health").clicked() {
                    self.navigate_to_page(Page::Health);
                }
            });
    }

    fn start_rotation_update(&mut self, image_key: String, new_rotation: u16) {
        // Mark as updating
        self.rotation_updating.insert(image_key.clone(), true);

        // Create shared state for the async operation
        let loading_state = Arc::new(Mutex::new(LoadState::Loading));
        self.rotation_promises.insert(image_key.clone(), loading_state.clone());

        // Spawn async task
        let request = RotateImageRequest {
            image_key: image_key.clone(),
            new_rotation,
        };

        wasm_bindgen_futures::spawn_local(async move {
            let result = update_image_rotation(request).await;

            let new_state = match result {
                Ok(response) => LoadState::Loaded(response),
                Err(error) => LoadState::Failed(error),
            };

            if let Ok(mut state) = loading_state.lock() {
                *state = new_state;
            }
        });
    }

    fn process_rotation_updates(&mut self) {
        // Collect completed updates
        let mut completed_updates = Vec::new();

        for (image_key, state_arc) in &self.rotation_promises {
            if let Ok(state) = state_arc.lock() {
                match &*state {
                    LoadState::Loaded(response) => {
                        completed_updates.push((image_key.clone(), Ok(response.clone())));
                    }
                    LoadState::Failed(error) => {
                        completed_updates.push((image_key.clone(), Err(error.clone())));
                    }
                    _ => {}
                }
            }
        }

        // Handle completed updates
        for (image_key, result) in completed_updates {
            self.rotation_promises.remove(&image_key);
            self.rotation_updating.remove(&image_key);

            match result {
                Ok(response) => {
                    // Show success toast
                    self.toast_message = Some((
                        format!("Rotation updated to {}°", response.new_rotation),
                        false
                    ));
                    self.toast_timer = 0.0;

                    // Reload images list from backend to get updated rotation metadata
                    self.images = AsyncResource::new();

                    // Clear texture so it gets recreated from cached bytes with new rotation
                    self.full_images.remove(&image_key);

                    // Clear thumbnail cache (cheaper to recreate)
                    self.thumbnails.remove(&image_key);
                }
                Err(error) => {
                    // Show error toast
                    self.toast_message = Some((
                        format!("Failed to update rotation: {}", error),
                        true
                    ));
                    self.toast_timer = 0.0;
                }
            }
        }
    }

    fn start_artifact_update(&mut self, artifact_id: u32, reason: String, date: Option<String>) {
        self.update_in_progress.insert(artifact_id, true);

        // Optimistically update local artifact data
        if let LoadState::Loaded(artifacts) = &mut self.artifacts.state {
            if let Some(a) = artifacts.iter_mut().find(|a| a.id == artifact_id) {
                a.updates.push(ArtifactUpdate {
                    author: "andrew".to_string(),
                    updated: String::new(),
                    reason: reason.clone(),
                    date: date.clone(),
                });
            }
        }

        let loading_state = Arc::new(Mutex::new(LoadState::Loading));
        self.update_promises.insert(artifact_id, loading_state.clone());

        let request = UpdateArtifactRequest {
            artifact_id,
            reason,
            date,
        };

        wasm_bindgen_futures::spawn_local(async move {
            let result = send_artifact_update(request).await;
            let new_state = match result {
                Ok(response) => LoadState::Loaded(response),
                Err(error) => LoadState::Failed(error),
            };
            if let Ok(mut state) = loading_state.lock() {
                *state = new_state;
            }
        });
    }

    fn process_artifact_updates(&mut self) {
        let mut completed = Vec::new();

        for (artifact_id, state_arc) in &self.update_promises {
            if let Ok(state) = state_arc.lock() {
                match &*state {
                    LoadState::Loaded(response) => {
                        completed.push((*artifact_id, Ok(response.clone())));
                    }
                    LoadState::Failed(error) => {
                        completed.push((*artifact_id, Err(error.clone())));
                    }
                    _ => {}
                }
            }
        }

        for (artifact_id, result) in completed {
            self.update_promises.remove(&artifact_id);
            self.update_in_progress.remove(&artifact_id);

            match result {
                Ok(response) => {
                    self.toast_message = Some(("Update saved".to_string(), false));
                    self.toast_timer = 0.0;
                    // Replace optimistic update with server response
                    if let LoadState::Loaded(artifacts) = &mut self.artifacts.state {
                        if let Some(a) = artifacts.iter_mut().find(|a| a.id == artifact_id) {
                            if let Some(last) = a.updates.last_mut() {
                                *last = response.update;
                            }
                        }
                    }
                }
                Err(error) => {
                    self.toast_message = Some((
                        format!("Failed to save update: {}", error),
                        true,
                    ));
                    self.toast_timer = 0.0;
                    // Remove optimistic update
                    if let LoadState::Loaded(artifacts) = &mut self.artifacts.state {
                        if let Some(a) = artifacts.iter_mut().find(|a| a.id == artifact_id) {
                            a.updates.pop();
                        }
                    }
                }
            }
        }
    }
}

/// Parse flexible date input into ISO 8601 partial date string.
/// Returns None for empty/whitespace input (clears date), Some(iso) for valid input.
/// Returns Err for unparseable input.
fn parse_fuzzy_date(input: &str) -> Result<Option<String>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }

    // Already ISO: "2020", "2020-12", "2020-12-05"
    if let Some(_) = try_parse_iso(input) {
        return Ok(Some(input.to_string()));
    }

    // MM/YYYY or MM-YYYY → YYYY-MM
    if input.len() >= 6 && input.len() <= 7 {
        if let Some(sep_pos) = input.find('/').or_else(|| input.find('-')) {
            let (left, right) = (&input[..sep_pos], &input[sep_pos + 1..]);
            if left.len() <= 2 && right.len() == 4 && right.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(month) = left.parse::<u8>() {
                    if (1..=12).contains(&month) {
                        return Ok(Some(format!("{}-{:02}", right, month)));
                    }
                }
            }
        }
    }

    // MM/DD/YYYY or MM-DD-YYYY → YYYY-MM-DD
    if input.len() >= 8 && input.len() <= 10 {
        let sep = if input.contains('/') { '/' } else { '-' };
        let parts: Vec<&str> = input.split(sep).collect();
        if parts.len() == 3 {
            let (p0, p1, p2) = (parts[0], parts[1], parts[2]);
            if p0.len() <= 2 && p1.len() <= 2 && p2.len() == 4 && p2.chars().all(|c| c.is_ascii_digit()) {
                if let (Ok(month), Ok(day)) = (p0.parse::<u8>(), p1.parse::<u8>()) {
                    if (1..=12).contains(&month) && (1..=31).contains(&day) {
                        return Ok(Some(format!("{}-{:02}-{:02}", p2, month, day)));
                    }
                }
            }
        }
    }

    // Month name patterns: "December 2020", "Dec 2020"
    let month_names = [
        ("january", "jan", 1), ("february", "feb", 2), ("march", "mar", 3),
        ("april", "apr", 4), ("may", "may", 5), ("june", "jun", 6),
        ("july", "jul", 7), ("august", "aug", 8), ("september", "sep", 9),
        ("october", "oct", 10), ("november", "nov", 11), ("december", "dec", 12),
    ];

    let lower = input.to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.len() == 2 {
        for &(full, abbr, num) in &month_names {
            if words[0] == full || words[0] == abbr {
                if let Ok(year) = words[1].parse::<u16>() {
                    if year >= 1000 {
                        return Ok(Some(format!("{}-{:02}", year, num)));
                    }
                }
            }
        }
    }

    Err(format!("Cannot parse date: '{}'", input))
}

fn try_parse_iso(s: &str) -> Option<()> {
    let bytes = s.as_bytes();
    if bytes.len() < 4 || !bytes[0..4].iter().all(|b| b.is_ascii_digit()) { return None; }
    if bytes.len() == 4 { return Some(()); }
    if bytes.len() < 7 || bytes[4] != b'-' { return None; }
    if !bytes[5..7].iter().all(|b| b.is_ascii_digit()) { return None; }
    let month: u8 = s[5..7].parse().ok()?;
    if !(1..=12).contains(&month) { return None; }
    if bytes.len() == 7 { return Some(()); }
    if bytes.len() != 10 || bytes[7] != b'-' { return None; }
    if !bytes[8..10].iter().all(|b| b.is_ascii_digit()) { return None; }
    let day: u8 = s[8..10].parse().ok()?;
    if !(1..=31).contains(&day) { return None; }
    Some(())
}

/// Format an ISO date string for human-friendly display.
fn format_date_for_display(iso: &str) -> String {
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let parts: Vec<&str> = iso.split('-').collect();
    match parts.len() {
        1 => parts[0].to_string(), // "2020"
        2 => {
            if let Ok(m) = parts[1].parse::<usize>() {
                if m >= 1 && m <= 12 {
                    return format!("{} {}", month_names[m - 1], parts[0]);
                }
            }
            iso.to_string()
        }
        3 => {
            if let (Ok(m), Ok(d)) = (parts[1].parse::<usize>(), parts[2].parse::<u8>()) {
                if m >= 1 && m <= 12 {
                    return format!("{} {}, {}", month_names[m - 1], d, parts[0]);
                }
            }
            iso.to_string()
        }
        _ => iso.to_string(),
    }
}

impl eframe::App for FamilyPhotosApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.images.process();
        self.artifacts.process();
        self.process_loaded_thumbnails(ctx);
        self.process_loaded_full_images(ctx);
        self.health.process();
        self.process_rotation_updates();
        self.process_artifact_updates();

        // Auto-load data based on current page/view
        let current_page = self.get_current_page();
        let viewing_image = self.get_selected_image().is_some();

        match current_page {
            Page::Images => {
                // Images page always needs images list
                if matches!(self.images.get(), LoadState::NotStarted) {
                    self.load_image_list(ctx);
                }
            }
            Page::Artifacts => {
                // Artifacts page needs artifacts list
                if matches!(self.artifacts.get(), LoadState::NotStarted) {
                    self.load_artifacts(ctx);
                }
                // Artifacts page (both list and detail) needs images list for rotation metadata
                if matches!(self.images.get(), LoadState::NotStarted) {
                    self.load_image_list(ctx);
                }
            }
            Page::Health => {
                if matches!(self.health.get(), LoadState::NotStarted) {
                    self.load_health(ctx);
                }
            }
        }

        // Image overlay (can appear over any page) needs images list for rotation metadata
        if viewing_image && matches!(self.images.get(), LoadState::NotStarted) {
            self.load_image_list(ctx);
        }

        // Update toast timer
        if self.toast_message.is_some() {
            self.toast_timer += ctx.input(|i| i.stable_dt) as f64;
            if self.toast_timer > 3.0 {
                self.toast_message = None;
                self.toast_timer = 0.0;
            }
        }

        // Render toast notification
        if let Some((message, is_error)) = &self.toast_message {
            let content_rect = ctx.available_rect();
            let toast_width = 400.0;
            let toast_height = 60.0;
            let toast_x = (content_rect.width() - toast_width) / 2.0;
            let toast_y = 20.0;

            egui::Window::new("toast")
                .title_bar(false)
                .resizable(false)
                .fixed_pos([toast_x, toast_y])
                .fixed_size([toast_width, toast_height])
                .frame(egui::Frame::window(&ctx.style()).fill(
                    if *is_error {
                        egui::Color32::from_rgb(200, 50, 50)
                    } else {
                        egui::Color32::from_rgb(50, 150, 50)
                    }
                ))
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(message)
                            .color(egui::Color32::WHITE)
                            .size(16.0));
                    });
                });
        }

        self.render_sidebar(ctx);

        // Show full image overlay if selected (parse from URL)
        if let Some(selected_id) = self.get_selected_image() {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let screen_rect = ctx.viewport_rect();

                    // Draw dark background
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(200),
                    );

                    // Detect click on background to close (but don't consume the click)
                    let bg_response = ui.interact(screen_rect, egui::Id::new("overlay_bg"), egui::Sense::click());

                    // Track old zoom for zoom-to-cursor
                    let old_zoom = self.zoom_controller.zoom;

                    // Right panel for controls and metadata
                    egui::SidePanel::right("image_controls")
                        .default_width(250.0)
                        .resizable(true)
                        .show_inside(ui, |ui| {
                            ui.add_space(10.0);

                            // Close button
                            if ui.button(egui::RichText::new("✕ Close").size(18.0)).clicked() {
                                self.close_image_view();
                            }

                            ui.add_space(20.0);
                            ui.separator();
                            ui.add_space(10.0);

                            // Zoom controls
                            ui.heading("Zoom");
                            ui.label(format!("{:.0}%", self.zoom_controller.zoom * 100.0));

                            // Handle pinch-to-zoom (trackpad gesture)
                            let zoom_delta = ui.input(|i| i.zoom_delta());
                            if zoom_delta != 1.0 {
                                self.zoom_controller.apply_zoom_delta(zoom_delta);
                            }

                            // Zoom slider
                            ui.add(egui::Slider::new(&mut self.zoom_controller.zoom, ZOOM_MIN..=ZOOM_MAX)
                                .text("")
                                .logarithmic(true));

                            // Reset zoom button
                            if ui.button("Reset Zoom (100%)").clicked() {
                                self.zoom_controller.reset();
                            }

                            ui.add_space(20.0);
                            ui.separator();
                            ui.add_space(10.0);

                            // Rotation controls
                            ui.heading("Rotation");

                            // Get current rotation for this image
                            let current_rotation = if let LoadState::Loaded(images) = self.images.get() {
                                images.iter()
                                    .find(|img| img.key == selected_id)
                                    .and_then(|img| img.rotation)
                                    .unwrap_or(0)
                            } else {
                                0
                            };

                            let is_updating = self.rotation_updating.get(&selected_id).copied().unwrap_or(false);

                            ui.add_enabled_ui(!is_updating, |ui| {
                                if ui.button("90°").clicked() {
                                    let new_rotation = (current_rotation + 90) % 360;
                                    self.start_rotation_update(selected_id.clone(), new_rotation);
                                }
                                if ui.button("180°").clicked() {
                                    let new_rotation = (current_rotation + 180) % 360;
                                    self.start_rotation_update(selected_id.clone(), new_rotation);
                                }
                                if ui.button("270°").clicked() {
                                    let new_rotation = (current_rotation + 270) % 360;
                                    self.start_rotation_update(selected_id.clone(), new_rotation);
                                }
                            });

                            if is_updating {
                                ui.spinner();
                                ui.label("Updating...");
                            }

                            ui.add_space(20.0);
                            ui.separator();
                            ui.add_space(10.0);

                            // Placeholder for EXIF data
                            ui.heading("Image Info");
                            ui.label("EXIF data will appear here");
                        });

                    // Main image area
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show_inside(ui, |ui| {
                            // Show full image or loading message
                            if let Some(texture) = self.full_images.get(&selected_id) {
                                let available_size = ui.available_size();

                                let texture_size = texture.size_vec2();
                                let base_scale = (available_size.x / texture_size.x)
                                    .min(available_size.y / texture_size.y)
                                    .min(1.0);

                                // Apply zoom
                                let scale = base_scale * self.zoom_controller.zoom;
                                let display_size = texture_size * scale;

                                // Calculate zoom change for scroll adjustment
                                let zoom_changed = old_zoom != self.zoom_controller.zoom;

                                // Use ScrollArea for panning with scroll
                                let scroll_id = egui::Id::new("image_scroll");

                                // Calculate new scroll offset if zoom changed (before showing ScrollArea)
                                let mut new_scroll_offset = self.zoom_controller.scroll_offset;
                                if zoom_changed {
                                    if let Some(pointer_pos) = ctx.pointer_hover_pos() {
                                        // Calculate mouse position in viewport
                                        let viewport_pos = ui.min_rect().min;
                                        let mouse_in_viewport = pointer_pos - viewport_pos;

                                        // Use zoom controller to calculate new offset
                                        new_scroll_offset = self.zoom_controller.calculate_zoom_to_cursor(
                                            old_zoom,
                                            self.zoom_controller.zoom,
                                            mouse_in_viewport,
                                            texture_size,
                                            base_scale,
                                            available_size,
                                        );
                                    }
                                }

                                // Create ScrollArea with scroll offset
                                let scroll_output = egui::ScrollArea::both()
                                    .auto_shrink([false, false])
                                    .id_salt(scroll_id)
                                    .scroll_offset(new_scroll_offset)
                                    .show(ui, |ui| {
                                        // Center the image if it's smaller than viewport
                                        let x_padding = ((ui.available_width() - display_size.x) / 2.0).max(0.0);
                                        let y_padding = ((ui.available_height() - display_size.y) / 2.0).max(0.0);

                                        ui.add_space(y_padding);
                                        ui.horizontal(|ui| {
                                            ui.add_space(x_padding);
                                            ui.image((texture.id(), display_size));
                                        });
                                    });

                                // Update our tracked scroll offset from the actual state
                                self.zoom_controller.scroll_offset = scroll_output.state.offset;
                            } else if let Some(err) = self.full_image_failures.get(&selected_id) {
                                ui.centered_and_justified(|ui| {
                                    ui.colored_label(
                                        egui::Color32::RED,
                                        egui::RichText::new(format!("Failed to load image: {}", err)).size(20.0),
                                    );
                                });
                            } else {
                                ui.centered_and_justified(|ui| {
                                    self.load_full_image(&selected_id, ctx);
                                    ui.label(egui::RichText::new("Loading...").size(24.0));
                                });
                            }
                        });

                    if bg_response.clicked() {
                        self.close_image_view();
                    }
                });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                match self.get_current_page() {
                    Page::Images => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);

                            ui.heading(egui::RichText::new("Family Photos").size(48.0).strong());
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("Click a photo to view full size").size(16.0));

                            ui.add_space(30.0);

                            match self.images.get().clone() {
                                LoadState::Loading => {
                                    ui.label(egui::RichText::new("Loading images...").size(20.0));
                                }
                                LoadState::Failed(err) => {
                                    ui.colored_label(
                                        egui::Color32::RED,
                                        egui::RichText::new(format!("Error: {}", err)).size(16.0),
                                    );
                                }
                                LoadState::Loaded(images) => {
                                    let thumbnail_height = 80.0;

                                    // Search bar
                                    ui.horizontal(|ui| {
                                        ui.label("Search:");
                                        ui.add(egui::TextEdit::singleline(&mut self.search_query)
                                            .hint_text("Filter by image key...")
                                            .desired_width(400.0));
                                        if !self.search_query.is_empty() {
                                            if ui.button("Clear").clicked() {
                                                self.search_query.clear();
                                            }
                                        }
                                    });
                                    ui.add_space(5.0);

                                    // Filter images by search query
                                    let filtered_images: Vec<&ImageMetadata> = if self.search_query.is_empty() {
                                        images.iter().collect()
                                    } else {
                                        let query = self.search_query.to_lowercase();
                                        images.iter()
                                            .filter(|img| img.key.to_lowercase().contains(&query))
                                            .collect()
                                    };

                                    if self.search_query.is_empty() {
                                        ui.label(format!("{} images", filtered_images.len()));
                                    } else {
                                        ui.label(format!("{} of {} images", filtered_images.len(), images.len()));
                                    }
                                    ui.add_space(10.0);

                                    // Load thumbnails only for visible rows + buffer of ~30 ahead
                                    let total = filtered_images.len();
                                    let (raw_vis_start, raw_vis_end) = self.images_visible_range;
                                    let (load_start, load_end) = if raw_vis_end > raw_vis_start && raw_vis_start < total {
                                        (raw_vis_start.saturating_sub(5), (raw_vis_end + 30).min(total))
                                    } else {
                                        (0, total.min(30))
                                    };

                                    if load_end > load_start {
                                        let keys_to_load: Vec<String> = filtered_images[load_start..load_end]
                                            .iter()
                                            .map(|img| img.key.clone())
                                            .collect();
                                        for chunk in keys_to_load.chunks(THUMBNAIL_BATCH_SIZE) {
                                            self.load_thumbnails_batch(chunk.to_vec(), ctx);
                                        }
                                    }

                                    use egui_extras::{TableBuilder, Column};

                                    let selected_key = self.get_selected_image();
                                    let mut min_visible = usize::MAX;
                                    let mut max_visible = 0usize;

                                    TableBuilder::new(ui)
                                        .striped(true)
                                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                        .column(Column::exact(100.0))
                                        .column(Column::remainder().at_least(150.0))
                                        .column(Column::exact(120.0))
                                        .column(Column::exact(100.0))
                                        .column(Column::exact(150.0))
                                        .header(30.0, |mut header| {
                                            header.col(|ui| { ui.strong("Thumbnail"); });
                                            header.col(|ui| { ui.strong("Key"); });
                                            header.col(|ui| { ui.strong("Date"); });
                                            header.col(|ui| { ui.strong("Size"); });
                                            header.col(|ui| { ui.strong("Tags"); });
                                        })
                                        .body(|body| {
                                            body.rows(thumbnail_height, filtered_images.len(), |mut row| {
                                                let idx = row.index();
                                                let image = filtered_images[idx];
                                                let image_key = image.key.clone();
                                                let is_selected = selected_key.as_ref() == Some(&image_key);
                                                row.set_selected(is_selected);

                                                min_visible = min_visible.min(idx);
                                                max_visible = max_visible.max(idx);

                                                let mut clicked = false;

                                                row.col(|ui| {
                                                    if let Some(texture) = self.thumbnails.get(&image.key) {
                                                        let texture_size = texture.size_vec2();
                                                        let aspect_ratio = texture_size.x / texture_size.y;
                                                        let display_width = thumbnail_height * aspect_ratio;
                                                        let display_size = Vec2::new(display_width.min(90.0), thumbnail_height);

                                                        let response = ui.add(egui::Image::new((texture.id(), display_size)).sense(egui::Sense::click().union(egui::Sense::hover())));

                                                        if response.clicked() {
                                                            clicked = true;
                                                        }

                                                        if response.hovered() {
                                                            let enlarged_height = 300.0;
                                                            let enlarged_width = enlarged_height * aspect_ratio;
                                                            let enlarged_size = Vec2::new(enlarged_width, enlarged_height);

                                                            let pointer_pos = ui.ctx().pointer_hover_pos().unwrap_or(response.rect.center());
                                                            let popup_pos = pointer_pos + egui::vec2(10.0, 10.0);

                                                            egui::Area::new(egui::Id::new(format!("hover_preview_{}", image.key)))
                                                                .fixed_pos(popup_pos)
                                                                .order(egui::Order::Tooltip)
                                                                .show(ui.ctx(), |ui| {
                                                                    egui::Frame::popup(ui.style())
                                                                        .show(ui, |ui| {
                                                                            ui.image((texture.id(), enlarged_size));
                                                                        });
                                                                });
                                                        }
                                                    } else if let Some(err) = self.thumbnail_failures.get(&image.key) {
                                                        ui.colored_label(
                                                            egui::Color32::RED,
                                                            format!("Error: {}", err)
                                                        );
                                                    } else {
                                                        ui.label("Loading...");
                                                    }
                                                });

                                                row.col(|ui| {
                                                    if ui.button(&image.key).clicked() {
                                                        clicked = true;
                                                    }
                                                });

                                                row.col(|ui| {
                                                    if ui.selectable_label(false, "—").clicked() {
                                                        clicked = true;
                                                    }
                                                });

                                                row.col(|ui| {
                                                    if ui.selectable_label(false, "—").clicked() {
                                                        clicked = true;
                                                    }
                                                });

                                                row.col(|ui| {
                                                    if ui.selectable_label(false, "—").clicked() {
                                                        clicked = true;
                                                    }
                                                });

                                                if clicked {
                                                    self.navigate_to_image(&image_key);
                                                }
                                            });
                                        });

                                    // Update visible range for next frame
                                    if min_visible <= max_visible {
                                        self.images_visible_range = (min_visible, max_visible);
                                    }
                                }
                                LoadState::NotStarted => {}
                            }
                        });
                    }
                    Page::Artifacts => {
                        // Check if we're viewing artifact detail (parse from URL)
                        if let Some(artifact_id) = self.get_selected_artifact() {
                            // Artifact detail view
                            let artifact = match &self.artifacts.state {
                                LoadState::Loaded(artifacts) => {
                                    artifacts.iter().find(|a| a.id == artifact_id).cloned()
                                }
                                _ => None,
                            };

                            if let Some(artifact) = artifact {
                                ui.add_space(20.0);

                                // Back button
                                if ui.button(egui::RichText::new("← Back to Artifacts").size(18.0)).clicked() {
                                    self.navigate_to_page(Page::Artifacts);
                                }

                                ui.add_space(20.0);

                                // Layout: Image grid on the left, attributes on the right
                                ui.horizontal(|ui| {
                                    // Left side: Image grid
                                    ui.vertical(|ui| {
                                        ui.heading(egui::RichText::new(format!("Artifact #{}", artifact_id)).size(32.0));
                                        ui.add_space(20.0);

                                        // Collect all image keys
                                        let mut image_keys = vec![artifact.images.front1.clone()];
                                        if let Some(front2) = &artifact.images.front2 {
                                            image_keys.push(front2.clone());
                                        }
                                        if let Some(back1) = &artifact.images.back1 {
                                            image_keys.push(back1.clone());
                                        }

                                        // Load thumbnails for all images
                                        self.load_thumbnails_batch(image_keys.clone(), ctx);

                                        // Display images in a grid (2 columns)
                                        let grid_size = 300.0;
                                        let spacing = 20.0;

                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(spacing, spacing);

                                            // Front 1
                                            ui.vertical(|ui| {
                                                ui.label(egui::RichText::new("Front 1").strong());
                                                if let Some(texture) = self.thumbnails.get(&artifact.images.front1) {
                                                    let texture_size = texture.size_vec2();

                                                    // Fit within bounds while maintaining aspect ratio
                                                    let scale = (grid_size / texture_size.x).min(grid_size / texture_size.y);
                                                    let display_size = Vec2::new(texture_size.x * scale, texture_size.y * scale);

                                                    if ui.add(egui::Image::new((texture.id(), display_size)).sense(egui::Sense::click())).clicked() {
                                                        self.navigate_to_image(&artifact.images.front1);
                                                    }
                                                } else {
                                                    ui.label("Loading...");
                                                }
                                            });

                                            // Front 2
                                            if let Some(front2) = &artifact.images.front2 {
                                                ui.vertical(|ui| {
                                                    ui.label(egui::RichText::new("Front 2").strong());
                                                    if let Some(texture) = self.thumbnails.get(front2) {
                                                        let texture_size = texture.size_vec2();

                                                        // Fit within bounds while maintaining aspect ratio
                                                        let scale = (grid_size / texture_size.x).min(grid_size / texture_size.y);
                                                        let display_size = Vec2::new(texture_size.x * scale, texture_size.y * scale);

                                                        if ui.add(egui::Image::new((texture.id(), display_size)).sense(egui::Sense::click())).clicked() {
                                                            self.navigate_to_image(front2);
                                                        }
                                                    } else {
                                                        ui.label("Loading...");
                                                    }
                                                });
                                            }

                                            // Back 1
                                            if let Some(back1) = &artifact.images.back1 {
                                                ui.vertical(|ui| {
                                                    ui.label(egui::RichText::new("Back 1").strong());
                                                    if let Some(texture) = self.thumbnails.get(back1) {
                                                        let texture_size = texture.size_vec2();

                                                        // Fit within bounds while maintaining aspect ratio
                                                        let scale = (grid_size / texture_size.x).min(grid_size / texture_size.y);
                                                        let display_size = Vec2::new(texture_size.x * scale, texture_size.y * scale);

                                                        if ui.add(egui::Image::new((texture.id(), display_size)).sense(egui::Sense::click())).clicked() {
                                                            self.navigate_to_image(back1);
                                                        }
                                                    } else {
                                                        ui.label("Loading...");
                                                    }
                                                });
                                            }
                                        });
                                    });

                                    ui.add_space(40.0);

                                    // Right side: Attributes
                                    ui.vertical(|ui| {
                                        ui.heading(egui::RichText::new("Attributes").size(24.0));
                                        ui.add_space(20.0);

                                        // Date display
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Date:").strong());
                                            if let Some(date) = artifact_date(&artifact) {
                                                ui.label(format_date_for_display(date));
                                            } else {
                                                ui.label("—");
                                            }
                                        });

                                        ui.add_space(10.0);
                                        ui.separator();
                                        ui.add_space(10.0);

                                        // Update form
                                        ui.label(egui::RichText::new("Log an update:").strong());
                                        ui.add_space(5.0);

                                        // Initialize edit states
                                        if !self.update_date_state.contains_key(&artifact_id) {
                                            self.update_date_state.insert(artifact_id, String::new());
                                        }
                                        if !self.update_reason_state.contains_key(&artifact_id) {
                                            self.update_reason_state.insert(artifact_id, String::new());
                                        }

                                        let is_updating = self.update_in_progress.get(&artifact_id).copied().unwrap_or(false);

                                        let mut save_clicked = false;
                                        let date_text = self.update_date_state.get_mut(&artifact_id).unwrap();
                                        let reason_text = self.update_reason_state.get_mut(&artifact_id).unwrap();
                                        ui.add_enabled_ui(!is_updating, |ui| {
                                            ui.horizontal(|ui| {
                                                ui.label("Date:");
                                                ui.add(
                                                    egui::TextEdit::singleline(date_text)
                                                        .hint_text("e.g. 2020, 12/2020, 02/05/2020")
                                                        .desired_width(200.0)
                                                );
                                            });

                                            ui.horizontal(|ui| {
                                                ui.label("Reason:");
                                                ui.add(
                                                    egui::TextEdit::singleline(reason_text)
                                                        .hint_text("Why are you making this change?")
                                                        .desired_width(200.0)
                                                );
                                            });

                                            if ui.button("Save").clicked() {
                                                save_clicked = true;
                                            }
                                        });

                                        if save_clicked {
                                            let date_input = self.update_date_state.get(&artifact_id)
                                                .cloned().unwrap_or_default();
                                            let reason_input = self.update_reason_state.get(&artifact_id)
                                                .cloned().unwrap_or_default();

                                            if date_input.trim().is_empty() {
                                                self.update_validation_error.insert(artifact_id, "Date is required".to_string());
                                            } else if reason_input.trim().is_empty() {
                                                self.update_validation_error.insert(artifact_id, "Reason is required".to_string());
                                            } else {
                                                match parse_fuzzy_date(&date_input) {
                                                    Ok(parsed_date) => {
                                                        self.update_validation_error.remove(&artifact_id);
                                                        self.update_date_state.remove(&artifact_id);
                                                        self.update_reason_state.remove(&artifact_id);
                                                        self.start_artifact_update(artifact_id, reason_input.trim().to_string(), parsed_date);
                                                    }
                                                    Err(msg) => {
                                                        self.update_validation_error.insert(artifact_id, msg);
                                                    }
                                                }
                                            }
                                        }

                                        if is_updating {
                                            ui.spinner();
                                        }

                                        if let Some(err) = self.update_validation_error.get(&artifact_id) {
                                            ui.colored_label(egui::Color32::RED, err);
                                        }

                                        ui.add_space(10.0);

                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Size:").strong());
                                            ui.label("—");
                                        });

                                        ui.add_space(10.0);

                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Tags:").strong());
                                            ui.label("—");
                                        });
                                    });
                                });
                            } else {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(40.0);
                                    ui.label("Artifact not found");
                                });
                            }
                        } else {
                            // Artifact list view
                            ui.vertical_centered(|ui| {
                                ui.add_space(40.0);
                                ui.heading(egui::RichText::new("Artifacts").size(48.0).strong());
                                ui.add_space(10.0);
                                ui.label(egui::RichText::new("Click an artifact to view details").size(16.0));
                                ui.add_space(30.0);

                                match self.artifacts.get().clone() {
                                    LoadState::Loading => {
                                        ui.label(egui::RichText::new("Loading artifacts...").size(20.0));
                                    }
                                    LoadState::Failed(err) => {
                                        ui.colored_label(
                                            egui::Color32::RED,
                                            egui::RichText::new(format!("Error: {}", err)).size(16.0),
                                        );
                                    }
                                    LoadState::Loaded(artifacts) => {
                                        let thumbnail_height = 80.0;

                                        // Load front1 thumbnails only for visible rows + buffer of ~30 ahead
                                        let total = artifacts.len();
                                        let (raw_vis_start, raw_vis_end) = self.artifacts_visible_range;
                                        let (load_start, load_end) = if raw_vis_end > raw_vis_start && raw_vis_start < total {
                                            (raw_vis_start.saturating_sub(5), (raw_vis_end + 30).min(total))
                                        } else {
                                            (0, total.min(30))
                                        };

                                        if load_end > load_start {
                                            let keys_to_load: Vec<String> = artifacts[load_start..load_end]
                                                .iter()
                                                .map(|a| a.images.front1.clone())
                                                .collect();
                                            for chunk in keys_to_load.chunks(THUMBNAIL_BATCH_SIZE) {
                                                self.load_thumbnails_batch(chunk.to_vec(), ctx);
                                            }
                                        }

                                        use egui_extras::{TableBuilder, Column};

                                        let selected_artifact = self.get_selected_artifact();
                                        let mut min_visible = usize::MAX;
                                        let mut max_visible = 0usize;

                                        TableBuilder::new(ui)
                                            .striped(true)
                                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                            .column(Column::exact(100.0))
                                            .column(Column::exact(120.0))
                                            .column(Column::exact(100.0))
                                            .column(Column::remainder().at_least(150.0))
                                            .header(30.0, |mut header| {
                                                header.col(|ui| { ui.strong("Thumbnail"); });
                                                header.col(|ui| { ui.strong("Date"); });
                                                header.col(|ui| { ui.strong("Size"); });
                                                header.col(|ui| { ui.strong("Tags"); });
                                            })
                                            .body(|body| {
                                                body.rows(thumbnail_height, artifacts.len(), |mut row| {
                                                    let idx = row.index();
                                                    let artifact = &artifacts[idx];
                                                    let front_key = &artifact.images.front1;
                                                    let artifact_id = artifact.id;
                                                    let is_selected = selected_artifact == Some(artifact_id);
                                                    row.set_selected(is_selected);

                                                    min_visible = min_visible.min(idx);
                                                    max_visible = max_visible.max(idx);

                                                    let mut clicked = false;

                                                    row.col(|ui| {
                                                        if let Some(texture) = self.thumbnails.get(front_key) {
                                                            let texture_size = texture.size_vec2();

                                                            let max_width = 90.0;
                                                            let max_height = thumbnail_height;
                                                            let scale_x = max_width / texture_size.x;
                                                            let scale_y = max_height / texture_size.y;
                                                            let scale = scale_x.min(scale_y);
                                                            let display_size = Vec2::new(texture_size.x * scale, texture_size.y * scale);

                                                            let response = ui.add(egui::Image::new((texture.id(), display_size)).sense(egui::Sense::click().union(egui::Sense::hover())));

                                                            if response.clicked() {
                                                                clicked = true;
                                                            }

                                                            if response.hovered() {
                                                                let aspect_ratio = texture_size.x / texture_size.y;
                                                                let enlarged_height = 300.0;
                                                                let enlarged_width = enlarged_height * aspect_ratio;
                                                                let enlarged_size = Vec2::new(enlarged_width, enlarged_height);

                                                                let pointer_pos = ui.ctx().pointer_hover_pos().unwrap_or(response.rect.center());
                                                                let popup_pos = pointer_pos + egui::vec2(10.0, 10.0);

                                                                egui::Area::new(egui::Id::new(format!("hover_preview_artifact_{}", artifact_id)))
                                                                    .fixed_pos(popup_pos)
                                                                    .order(egui::Order::Tooltip)
                                                                    .show(ui.ctx(), |ui| {
                                                                        egui::Frame::popup(ui.style())
                                                                            .show(ui, |ui| {
                                                                                ui.image((texture.id(), enlarged_size));
                                                                            });
                                                                    });
                                                            }
                                                        } else if let Some(err) = self.thumbnail_failures.get(front_key) {
                                                            ui.colored_label(
                                                                egui::Color32::RED,
                                                                format!("Error: {}", err)
                                                            );
                                                        } else {
                                                            ui.label("Loading...");
                                                        }
                                                    });

                                                    row.col(|ui| {
                                                        let date_label = artifact_date(artifact)
                                                            .map(format_date_for_display)
                                                            .unwrap_or_else(|| "—".to_string());
                                                        if ui.selectable_label(false, &date_label).clicked() {
                                                            clicked = true;
                                                        }
                                                    });

                                                    row.col(|ui| {
                                                        if ui.selectable_label(false, "—").clicked() {
                                                            clicked = true;
                                                        }
                                                    });

                                                    row.col(|ui| {
                                                        if ui.selectable_label(false, "—").clicked() {
                                                            clicked = true;
                                                        }
                                                    });

                                                    if clicked {
                                                        self.navigate_to_artifact(artifact_id);
                                                    }
                                                });
                                            });

                                        // Update visible range for next frame
                                        if min_visible <= max_visible {
                                            self.artifacts_visible_range = (min_visible, max_visible);
                                        }
                                    }
                                    LoadState::NotStarted => {}
                                }
                            });
                        }
                    }
                    Page::Health => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);

                            ui.heading(egui::RichText::new("Health Check").size(48.0).strong());

                            ui.add_space(30.0);

                            if matches!(self.health.get(), LoadState::NotStarted) {
                                self.load_health(ctx);
                            }

                            match self.health.get().clone() {
                                LoadState::Loading => {
                                    ui.label(egui::RichText::new("Loading...").size(20.0));
                                }
                                LoadState::Failed(err) => {
                                    ui.colored_label(
                                        egui::Color32::RED,
                                        egui::RichText::new(format!("Error: {}", err)).size(16.0),
                                    );
                                }
                                LoadState::Loaded(health) => {
                                    ui.label(egui::RichText::new(format!("Status: {}", health.status)).size(24.0));
                                    ui.add_space(10.0);
                                    ui.label(egui::RichText::new(format!("Message: {}", health.message)).size(18.0));
                                }
                                LoadState::NotStarted => {}
                            }
                        });
                    }
                }
            });
        }
    }
}

async fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let full_url = format!("{}{}", API_BASE_URL, url);
    let response = ehttp::fetch_async(ehttp::Request::get(&full_url))
        .await
        .map_err(|e| format!("Fetch failed: {}", e))?;

    if !response.ok {
        return Err(format!(
            "HTTP error: {} {}",
            response.status, response.status_text
        ));
    }

    serde_json::from_slice(&response.bytes).map_err(|e| format!("JSON parse error: {}", e))
}

async fn fetch_image_from_url(url: &str) -> Result<Vec<u8>, String> {
    let response = ehttp::fetch_async(ehttp::Request::get(url))
        .await
        .map_err(|e| format!("Fetch failed: {}", e))?;

    if !response.ok {
        return Err(format!(
            "HTTP error: {} {}",
            response.status, response.status_text
        ));
    }

    Ok(response.bytes)
}

async fn fetch_thumbnails_batch(keys: &[String]) -> Result<HashMap<String, Vec<u8>>, String> {
    let full_url = format!("{}/api/images/thumbnails", API_BASE_URL);
    let request_body = ThumbnailBatchRequest {
        keys: keys.to_vec(),
    };

    let body_json = serde_json::to_string(&request_body)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;

    let mut request = ehttp::Request::post(full_url, body_json.into_bytes());
    request.headers.insert("Content-Type".to_string(), "application/json".to_string());

    let response = ehttp::fetch_async(request)
        .await
        .map_err(|e| format!("Fetch failed: {}", e))?;

    if !response.ok {
        return Err(format!(
            "HTTP error: {} {}",
            response.status, response.status_text
        ));
    }

    let batch_response: ThumbnailBatchResponse = serde_json::from_slice(&response.bytes)
        .map_err(|e| format!("JSON parse error: {}", e))?;

    Ok(batch_response.thumbnails)
}

async fn update_image_rotation(request: RotateImageRequest) -> Result<RotateImageResponse, String> {
    let full_url = format!("{}/api/images/rotate", API_BASE_URL);

    let body_json = serde_json::to_string(&request)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;

    let mut http_request = ehttp::Request::post(full_url, body_json.into_bytes());
    http_request.headers.insert("Content-Type".to_string(), "application/json".to_string());

    let response = ehttp::fetch_async(http_request)
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok {
        // Try to parse error response
        if let Ok(error_response) = serde_json::from_slice::<ErrorResponse>(&response.bytes) {
            return Err(format!("{}", error_response.error));
        }
        return Err(format!(
            "HTTP error: {} {}",
            response.status, response.status_text
        ));
    }

    let rotate_response: RotateImageResponse = serde_json::from_slice(&response.bytes)
        .map_err(|e| format!("JSON parse error: {}", e))?;

    Ok(rotate_response)
}

async fn send_artifact_update(request: UpdateArtifactRequest) -> Result<UpdateArtifactResponse, String> {
    let full_url = format!("{}/api/artifacts/update", API_BASE_URL);

    let body_json = serde_json::to_string(&request)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;

    let mut http_request = ehttp::Request::post(full_url, body_json.into_bytes());
    http_request.headers.insert("Content-Type".to_string(), "application/json".to_string());

    let response = ehttp::fetch_async(http_request)
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.ok {
        if let Ok(error_response) = serde_json::from_slice::<ErrorResponse>(&response.bytes) {
            return Err(error_response.error);
        }
        return Err(format!(
            "HTTP error: {} {}",
            response.status, response.status_text
        ));
    }

    serde_json::from_slice(&response.bytes)
        .map_err(|e| format!("JSON parse error: {}", e))
}

/// Decode image using Rust image crate (used for thumbnails).
fn load_image_from_bytes(bytes: &[u8], rotation: Option<u16>) -> Option<ColorImage> {
    match image::load_from_memory(bytes) {
        Ok(mut dynamic_image) => {
            if let Some(degrees) = rotation {
                dynamic_image = match degrees {
                    90 => dynamic_image.rotate90(),
                    180 => dynamic_image.rotate180(),
                    270 => dynamic_image.rotate270(),
                    _ => dynamic_image,
                };
            }

            // Safety net: downscale if exceeding GPU max texture size
            let (w, h) = (dynamic_image.width(), dynamic_image.height());
            if w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
                let scale = (MAX_TEXTURE_SIDE as f64 / w as f64)
                    .min(MAX_TEXTURE_SIDE as f64 / h as f64);
                let new_w = (w as f64 * scale) as u32;
                let new_h = (h as f64 * scale) as u32;
                dynamic_image = dynamic_image.resize_exact(
                    new_w, new_h, image::imageops::FilterType::Triangle,
                );
            }

            let rgba_image = dynamic_image.to_rgba8();
            let size = [rgba_image.width() as usize, rgba_image.height() as usize];
            let pixels = rgba_image.into_raw();
            Some(ColorImage::from_rgba_unmultiplied(size, &pixels))
        }
        Err(e) => {
            web_sys::console::error_1(&format!("Failed to load image: {}", e).into());
            None
        }
    }
}

/// Decode a full image using the browser's native decoder (fast, non-blocking).
/// Falls back to Rust image crate for formats the browser can't handle (e.g. TIFF).
async fn decode_full_image(bytes: &[u8], rotation: Option<u16>) -> Result<ColorImage, String> {
    match decode_image_native(bytes, rotation).await {
        Ok(img) => Ok(img),
        Err(_) => {
            // Fallback to Rust image crate (slower but handles more formats)
            load_image_from_bytes(bytes, rotation)
                .ok_or_else(|| "Failed to decode image".to_string())
        }
    }
}

/// Browser-native image decode via createImageBitmap + canvas pixel extraction.
/// The heavy decode work happens in browser internals (native code, off main thread).
async fn decode_image_native(bytes: &[u8], rotation: Option<u16>) -> Result<ColorImage, String> {
    use wasm_bindgen::JsCast;

    // Create Blob from raw bytes
    let uint8_array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&uint8_array);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
        .map_err(|e| format!("Blob creation failed: {:?}", e))?;

    // Browser-native async image decode
    let window = web_sys::window().ok_or("No window")?;
    let promise = window.create_image_bitmap_with_blob(&blob)
        .map_err(|e| format!("createImageBitmap failed: {:?}", e))?;
    let bitmap_js = wasm_bindgen_futures::JsFuture::from(promise).await
        .map_err(|e| format!("Image decode failed: {:?}", e))?;
    let bitmap: web_sys::ImageBitmap = bitmap_js.dyn_into()
        .map_err(|_| "Not an ImageBitmap".to_string())?;

    let bw = bitmap.width();
    let bh = bitmap.height();
    let degrees = rotation.unwrap_or(0);

    // Output dimensions after rotation
    let (out_w, out_h) = match degrees {
        90 | 270 => (bh, bw),
        _ => (bw, bh),
    };

    // Scale down if exceeding GPU max texture size
    let scale = if out_w > MAX_TEXTURE_SIDE || out_h > MAX_TEXTURE_SIDE {
        (MAX_TEXTURE_SIDE as f64 / out_w as f64).min(MAX_TEXTURE_SIDE as f64 / out_h as f64)
    } else {
        1.0
    };
    let canvas_w = ((out_w as f64) * scale) as u32;
    let canvas_h = ((out_h as f64) * scale) as u32;

    // Create temporary canvas for pixel extraction
    let document = window.document().ok_or("No document")?;
    let canvas = document.create_element("canvas")
        .map_err(|e| format!("createElement failed: {:?}", e))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "Not a canvas element".to_string())?;
    canvas.set_width(canvas_w);
    canvas.set_height(canvas_h);

    let ctx = canvas.get_context("2d")
        .map_err(|e| format!("getContext failed: {:?}", e))?
        .ok_or("No 2d context")?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .map_err(|_| "Not a 2d context".to_string())?;

    // Apply rotation transform, then draw scaled bitmap
    let draw_w = bw as f64 * scale;
    let draw_h = bh as f64 * scale;

    match degrees {
        90 => {
            let _ = ctx.translate(canvas_w as f64, 0.0);
            let _ = ctx.rotate(std::f64::consts::FRAC_PI_2);
        }
        180 => {
            let _ = ctx.translate(canvas_w as f64, canvas_h as f64);
            let _ = ctx.rotate(std::f64::consts::PI);
        }
        270 => {
            let _ = ctx.translate(0.0, canvas_h as f64);
            let _ = ctx.rotate(-std::f64::consts::FRAC_PI_2);
        }
        _ => {}
    }

    ctx.draw_image_with_image_bitmap_and_dw_and_dh(&bitmap, 0.0, 0.0, draw_w, draw_h)
        .map_err(|e| format!("drawImage failed: {:?}", e))?;

    // Extract RGBA pixels
    let image_data = ctx.get_image_data(0.0, 0.0, canvas_w as f64, canvas_h as f64)
        .map_err(|e| format!("getImageData failed: {:?}", e))?;
    let pixels = image_data.data().to_vec();

    Ok(ColorImage::from_rgba_unmultiplied(
        [canvas_w as usize, canvas_h as usize],
        &pixels,
    ))
}
