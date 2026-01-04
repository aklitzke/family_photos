use common::{Artifact, ArtifactListResponse, HealthResponse, ImageListResponse, ImageMetadata, PresignedUrlResponse, ThumbnailBatchRequest, ThumbnailBatchResponse};
use eframe::egui::{self, ColorImage, TextureHandle};
use eframe::epaint::Vec2;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

const API_BASE_URL: &str = if cfg!(debug_assertions) {
    "http://localhost:8787"
} else {
    ""
};

// UI Constants
const ZOOM_MIN: f32 = 0.1;
const ZOOM_MAX: f32 = 50.0;
const ZOOM_DEFAULT: f32 = 1.0;
const THUMBNAIL_BATCH_SIZE: usize = 10;

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
    current_page: Page,
    images: AsyncResource<Vec<ImageMetadata>>,
    artifacts: AsyncResource<Vec<Artifact>>,
    thumbnails: HashMap<String, TextureHandle>,
    thumbnail_loading: HashMap<String, Arc<Mutex<LoadState<Vec<u8>>>>>,
    thumbnail_failures: HashMap<String, String>,  // Track permanent failures
    selected_image: Option<String>,
    selected_artifact: Option<usize>,  // Index of selected artifact for detail view
    full_images: HashMap<String, TextureHandle>,
    full_images_loading: HashMap<String, Arc<Mutex<LoadState<Vec<u8>>>>>,
    full_image_failures: HashMap<String, String>,  // Track permanent failures
    zoom_controller: ZoomController,
    health: AsyncResource<HealthResponse>,
}

impl FamilyPhotosApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            current_page: Page::Artifacts,
            images: AsyncResource::new(),
            artifacts: AsyncResource::new(),
            thumbnails: HashMap::new(),
            thumbnail_loading: HashMap::new(),
            thumbnail_failures: HashMap::new(),
            selected_image: None,
            selected_artifact: None,
            full_images: HashMap::new(),
            full_images_loading: HashMap::new(),
            full_image_failures: HashMap::new(),
            zoom_controller: ZoomController::new(),
            health: AsyncResource::new(),
        };

        // Parse initial URL to set state
        app.parse_url();

        // Update URL to reflect the actual page (e.g., "/" -> "/artifacts")
        app.update_url();

        app
    }

    fn parse_url(&mut self) {
        let window = web_sys::window().unwrap();
        let location = window.location();
        let pathname = location.pathname().unwrap_or_default();

        // Parse the path
        let parts: Vec<&str> = pathname.trim_matches('/').split('/').collect();

        match parts.get(0) {
            Some(&"images") => {
                self.current_page = Page::Images;
                // Check for image key in query string
                if let Ok(search) = location.search() {
                    if let Some(key) = Self::parse_query_param(&search, "key") {
                        self.selected_image = Some(key);
                    }
                }
            }
            Some(&"health") => {
                self.current_page = Page::Health;
            }
            Some(&"artifacts") | Some(&"") | None => {
                self.current_page = Page::Artifacts;
                // Check if there's an artifact ID
                if let Some(id_str) = parts.get(1) {
                    if let Ok(id) = id_str.parse::<usize>() {
                        self.selected_artifact = Some(id);
                    }
                }
            }
            _ => {
                self.current_page = Page::Artifacts;
            }
        }
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

    fn update_url(&self) {
        let window = web_sys::window().unwrap();
        let history = window.history().unwrap();

        // Priority: selected_image > selected_artifact > current_page
        let path = if let Some(key) = &self.selected_image {
            format!("/images?key={}", urlencoding::encode(key))
        } else {
            match self.current_page {
                Page::Images => "/images".to_string(),
                Page::Artifacts => {
                    if let Some(idx) = self.selected_artifact {
                        format!("/artifacts/{}", idx)
                    } else {
                        "/artifacts".to_string()
                    }
                }
                Page::Health => "/health".to_string(),
            }
        };

        // Update URL without reloading the page
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

    fn load_thumbnail(&mut self, key: &str, ctx: &egui::Context) {
        // Don't retry if already loaded, loading, or permanently failed
        if self.thumbnails.contains_key(key)
            || self.thumbnail_loading.contains_key(key)
            || self.thumbnail_failures.contains_key(key) {
            return;
        }

        let loading_state = Arc::new(Mutex::new(LoadState::Loading));
        self.thumbnail_loading.insert(key.to_string(), loading_state.clone());

        let key_encoded = urlencoding::encode(key).to_string();
        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_image(&format!("/api/images/thumbnail?key={}", key_encoded)).await {
                Ok(image_data) => {
                    *loading_state.lock().unwrap() = LoadState::Loaded(image_data);
                    ctx_clone.request_repaint();
                }
                Err(e) => {
                    *loading_state.lock().unwrap() = LoadState::Failed(e);
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

        let loading_state = Arc::new(Mutex::new(LoadState::Loading));
        self.full_images_loading.insert(key.to_string(), loading_state.clone());

        let key_encoded = urlencoding::encode(key).to_string();
        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            // First, get the presigned URL from the API
            match fetch_json::<PresignedUrlResponse>(&format!("/api/images/full?key={}", key_encoded)).await {
                Ok(response) => {
                    // Then fetch the actual image from the presigned URL
                    match fetch_image_from_url(&response.url).await {
                        Ok(image_data) => {
                            *loading_state.lock().unwrap() = LoadState::Loaded(image_data);
                            ctx_clone.request_repaint();
                        }
                        Err(e) => {
                            *loading_state.lock().unwrap() = LoadState::Failed(format!("Failed to fetch image from R2: {}", e));
                            ctx_clone.request_repaint();
                        }
                    }
                }
                Err(e) => {
                    *loading_state.lock().unwrap() = LoadState::Failed(format!("Failed to get presigned URL: {}", e));
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

        // Get rotation info from loaded images
        let rotation_map: HashMap<String, Option<u16>> = if let LoadState::Loaded(images) = self.images.get() {
            images.iter().map(|img| (img.key.clone(), img.rotation)).collect()
        } else {
            HashMap::new()
        };

        for (id, loading_state) in &self.full_images_loading {
            let state = loading_state.lock().unwrap();
            match &*state {
                LoadState::Loaded(data) => {
                    let rotation = rotation_map.get(id).and_then(|r| *r);
                    if let Some(color_image) = load_image_from_bytes(data, rotation) {
                        let texture = ctx.load_texture(
                            format!("full_image_{}", id),
                            color_image,
                            Default::default(),
                        );
                        self.full_images.insert(id.clone(), texture);
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
        self.selected_image = None;
        self.zoom_controller.reset();
        self.update_url();
    }

    fn render_sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(150.0)
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.heading("Family Photos");
                ui.add_space(20.0);

                if ui.selectable_label(self.current_page == Page::Artifacts, "Artifacts").clicked() {
                    self.current_page = Page::Artifacts;
                    self.selected_artifact = None;
                    self.update_url();
                }

                if ui.selectable_label(self.current_page == Page::Images, "Images").clicked() {
                    self.current_page = Page::Images;
                    self.selected_image = None;
                    self.update_url();
                }

                if ui.selectable_label(self.current_page == Page::Health, "Health").clicked() {
                    self.current_page = Page::Health;
                    self.update_url();
                }
            });
    }
}

impl eframe::App for FamilyPhotosApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.images.process();
        self.artifacts.process();
        self.process_loaded_thumbnails(ctx);
        self.process_loaded_full_images(ctx);
        self.health.process();

        self.render_sidebar(ctx);

        // Show full image overlay if selected
        if let Some(selected_id) = self.selected_image.clone() {
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
                match self.current_page {
                    Page::Images => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);

                            ui.heading(egui::RichText::new("Family Photos").size(48.0).strong());
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("Click a photo to view full size").size(16.0));

                            ui.add_space(30.0);

                            if matches!(self.images.get(), LoadState::NotStarted) {
                                self.load_image_list(ctx);
                            }

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

                                    // Load thumbnails in batches to avoid overwhelming the server
                                    let image_keys: Vec<String> = images.iter().map(|img| img.key.clone()).collect();

                                    // Split into chunks and load each batch
                                    for chunk in image_keys.chunks(THUMBNAIL_BATCH_SIZE) {
                                        self.load_thumbnails_batch(chunk.to_vec(), ctx);
                                    }

                                    use egui_extras::{TableBuilder, Column};

                                    TableBuilder::new(ui)
                                        .striped(true)
                                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                        .column(Column::exact(100.0))
                                        .column(Column::remainder().at_least(150.0))
                                        .column(Column::exact(120.0))
                                        .column(Column::exact(100.0))
                                        .column(Column::exact(150.0))
                                        .header(30.0, |mut header| {
                                            header.col(|ui| {
                                                ui.strong("Thumbnail");
                                            });
                                            header.col(|ui| {
                                                ui.strong("Key");
                                            });
                                            header.col(|ui| {
                                                ui.strong("Date");
                                            });
                                            header.col(|ui| {
                                                ui.strong("Size");
                                            });
                                            header.col(|ui| {
                                                ui.strong("Tags");
                                            });
                                        })
                                        .body(|mut body| {
                                            for image in &images {
                                                let image_key = image.key.clone();
                                                let is_selected = self.selected_image.as_ref() == Some(&image_key);

                                                body.row(thumbnail_height, |mut row| {
                                                    row.set_selected(is_selected);

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
                                                        self.selected_image = Some(image_key.clone());
                                                        self.update_url();
                                                    }
                                                });
                                            }
                                        });
                                }
                                LoadState::NotStarted => {}
                            }
                        });
                    }
                    Page::Artifacts => {
                        // Check if we're viewing artifact detail
                        if let Some(artifact_idx) = self.selected_artifact {
                            // Artifact detail view
                            let artifact = match &self.artifacts.state {
                                LoadState::Loaded(artifacts) => {
                                    if artifact_idx < artifacts.len() {
                                        Some(artifacts[artifact_idx].clone())
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            };

                            if let Some(artifact) = artifact {
                                ui.add_space(20.0);

                                // Back button
                                if ui.button(egui::RichText::new("← Back to Artifacts").size(18.0)).clicked() {
                                    self.selected_artifact = None;
                                    self.update_url();
                                }

                                ui.add_space(20.0);

                                // Layout: Image grid on the left, attributes on the right
                                ui.horizontal(|ui| {
                                    // Left side: Image grid
                                    ui.vertical(|ui| {
                                        ui.heading(egui::RichText::new(format!("Artifact {}", artifact_idx + 1)).size(32.0));
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
                                                        self.selected_image = Some(artifact.images.front1.clone());
                                                        self.update_url();
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
                                                            self.selected_image = Some(front2.clone());
                                                            self.update_url();
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
                                                            self.selected_image = Some(back1.clone());
                                                            self.update_url();
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

                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new("Date:").strong());
                                            ui.label("—");
                                        });

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

                                if matches!(self.artifacts.get(), LoadState::NotStarted) {
                                    self.load_artifacts(ctx);
                                }

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

                                        // Load front1 thumbnails in batches
                                        let front_keys: Vec<String> = artifacts.iter()
                                            .map(|artifact| artifact.images.front1.clone())
                                            .collect();

                                        // Split into chunks and load each batch
                                        for chunk in front_keys.chunks(THUMBNAIL_BATCH_SIZE) {
                                            self.load_thumbnails_batch(chunk.to_vec(), ctx);
                                        }

                                        use egui_extras::{TableBuilder, Column};

                                        TableBuilder::new(ui)
                                            .striped(true)
                                            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                                            .column(Column::exact(100.0))
                                            .column(Column::exact(120.0))
                                            .column(Column::exact(100.0))
                                            .column(Column::remainder().at_least(150.0))
                                            .header(30.0, |mut header| {
                                                header.col(|ui| {
                                                    ui.strong("Thumbnail");
                                                });
                                                header.col(|ui| {
                                                    ui.strong("Date");
                                                });
                                                header.col(|ui| {
                                                    ui.strong("Size");
                                                });
                                                header.col(|ui| {
                                                    ui.strong("Tags");
                                                });
                                            })
                                            .body(|mut body| {
                                                for (idx, artifact) in artifacts.iter().enumerate() {
                                                    let front_key = &artifact.images.front1;
                                                    let is_selected = self.selected_artifact == Some(idx);

                                                    body.row(thumbnail_height, |mut row| {
                                                        row.set_selected(is_selected);

                                                        let mut clicked = false;

                                                        row.col(|ui| {
                                                            if let Some(texture) = self.thumbnails.get(front_key) {
                                                                let texture_size = texture.size_vec2();

                                                                // Fit within bounds while maintaining aspect ratio
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

                                                                    egui::Area::new(egui::Id::new(format!("hover_preview_artifact_{}", idx)))
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
                                                            self.selected_artifact = Some(idx);
                                                            self.update_url();
                                                        }
                                                    });
                                                }
                                            });
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

async fn fetch_image(url: &str) -> Result<Vec<u8>, String> {
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

    Ok(response.bytes)
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

fn load_image_from_bytes(bytes: &[u8], rotation: Option<u16>) -> Option<ColorImage> {
    match image::load_from_memory(bytes) {
        Ok(mut dynamic_image) => {
            // Apply rotation if specified
            if let Some(degrees) = rotation {
                dynamic_image = match degrees {
                    90 => dynamic_image.rotate90(),
                    180 => dynamic_image.rotate180(),
                    270 => dynamic_image.rotate270(),
                    _ => dynamic_image, // 0 or invalid, no rotation
                };
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
