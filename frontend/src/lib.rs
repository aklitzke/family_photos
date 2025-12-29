use common::{HealthResponse, ImageListResponse, ImageMetadata};
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

#[derive(Clone, PartialEq)]
enum Page {
    Images,
    Health,
}

#[derive(Clone)]
enum LoadState<T: Clone> {
    NotStarted,
    Loading,
    Loaded(T),
    Failed(String),
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
    images: LoadState<Vec<ImageMetadata>>,
    images_loading: Option<Arc<Mutex<LoadState<Vec<ImageMetadata>>>>>,
    thumbnails: HashMap<String, TextureHandle>,
    thumbnail_loading: HashMap<String, Arc<Mutex<LoadState<Vec<u8>>>>>,
    selected_image: Option<String>,
    full_image: Option<TextureHandle>,
    full_image_loading: Option<Arc<Mutex<LoadState<Vec<u8>>>>>,
    health: LoadState<HealthResponse>,
    health_loading: Option<Arc<Mutex<LoadState<HealthResponse>>>>,
}

impl FamilyPhotosApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            current_page: Page::Images,
            images: LoadState::NotStarted,
            images_loading: None,
            thumbnails: HashMap::new(),
            thumbnail_loading: HashMap::new(),
            selected_image: None,
            full_image: None,
            full_image_loading: None,
            health: LoadState::NotStarted,
            health_loading: None,
        }
    }

    fn load_image_list(&mut self, ctx: &egui::Context) {
        if self.images_loading.is_some() || !matches!(self.images, LoadState::NotStarted) {
            return;
        }

        self.images = LoadState::Loading;
        let images_state = Arc::new(Mutex::new(LoadState::<Vec<ImageMetadata>>::Loading));
        self.images_loading = Some(images_state.clone());
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

    fn process_loaded_images(&mut self) {
        let should_update = if let Some(loading_state) = &self.images_loading {
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
            self.images = new_state;
            self.images_loading = None;
        }
    }

    fn load_thumbnail(&mut self, id: &str, ctx: &egui::Context) {
        if self.thumbnails.contains_key(id) || self.thumbnail_loading.contains_key(id) {
            return;
        }

        let loading_state = Arc::new(Mutex::new(LoadState::Loading));
        self.thumbnail_loading.insert(id.to_string(), loading_state.clone());

        let id_clone = id.to_string();
        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_image(&format!("/api/images/thumbnail/{}", id_clone)).await {
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

    fn load_full_image(&mut self, id: &str, ctx: &egui::Context) {
        if self.full_image_loading.is_some() {
            return;
        }

        let loading_state = Arc::new(Mutex::new(LoadState::Loading));
        self.full_image_loading = Some(loading_state.clone());

        let id_clone = id.to_string();
        let ctx_clone = ctx.clone();

        wasm_bindgen_futures::spawn_local(async move {
            match fetch_image(&format!("/api/images/full/{}", id_clone)).await {
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

    fn process_loaded_thumbnails(&mut self, ctx: &egui::Context) {
        let mut completed = Vec::new();

        for (id, loading_state) in &self.thumbnail_loading {
            let state = loading_state.lock().unwrap();
            match &*state {
                LoadState::Loaded(data) => {
                    if let Some(color_image) = load_image_from_bytes(data) {
                        let texture = ctx.load_texture(
                            format!("thumbnail_{}", id),
                            color_image,
                            Default::default(),
                        );
                        self.thumbnails.insert(id.clone(), texture);
                    }
                    completed.push(id.clone());
                }
                LoadState::Failed(_) => {
                    completed.push(id.clone());
                }
                _ => {}
            }
        }

        for id in completed {
            self.thumbnail_loading.remove(&id);
        }
    }

    fn process_loaded_full_image(&mut self, ctx: &egui::Context) {
        let should_update = if let Some(loading_state) = &self.full_image_loading {
            let state = loading_state.lock().unwrap();
            match &*state {
                LoadState::Loaded(data) => {
                    if let Some(color_image) = load_image_from_bytes(data) {
                        let texture = ctx.load_texture(
                            "full_image",
                            color_image,
                            Default::default(),
                        );
                        Some(Some(texture))
                    } else {
                        Some(None)
                    }
                }
                LoadState::Failed(_) => Some(None),
                _ => None,
            }
        } else {
            None
        };

        if let Some(new_texture) = should_update {
            self.full_image = new_texture;
            self.full_image_loading = None;
        }
    }

    fn load_health(&mut self, ctx: &egui::Context) {
        if self.health_loading.is_some() || !matches!(self.health, LoadState::NotStarted) {
            return;
        }

        self.health = LoadState::Loading;
        let health_state = Arc::new(Mutex::new(LoadState::<HealthResponse>::Loading));
        self.health_loading = Some(health_state.clone());
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

    fn process_loaded_health(&mut self) {
        let should_update = if let Some(loading_state) = &self.health_loading {
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
            self.health = new_state;
            self.health_loading = None;
        }
    }
}

impl eframe::App for FamilyPhotosApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_loaded_images();
        self.process_loaded_thumbnails(ctx);
        self.process_loaded_full_image(ctx);
        self.process_loaded_health();

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .default_width(150.0)
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.heading("Family Photos");
                ui.add_space(20.0);

                if ui.selectable_label(self.current_page == Page::Images, "Images").clicked() {
                    self.current_page = Page::Images;
                }

                if ui.selectable_label(self.current_page == Page::Health, "Health").clicked() {
                    self.current_page = Page::Health;
                }
            });

        // Show full image overlay if selected
        if let Some(selected_id) = self.selected_image.clone() {
            // Dark background overlay
            egui::Area::new(egui::Id::new("overlay_background"))
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    let screen_rect = ctx.viewport_rect();
                    let painter = ui.painter();
                    painter.rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(200),
                    );

                    // Detect click on background to close
                    let response = ui.allocate_rect(screen_rect, egui::Sense::click());
                    if response.clicked() {
                        self.selected_image = None;
                        self.full_image = None;
                        self.full_image_loading = None;
                    }
                });

            // Center panel for full image
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.0);

                        // Close button
                        if ui.button(egui::RichText::new("✕ Close").size(20.0)).clicked() {
                            self.selected_image = None;
                            self.full_image = None;
                            self.full_image_loading = None;
                        }

                        ui.add_space(20.0);

                        // Show full image or loading message
                        if let Some(texture) = &self.full_image {
                            let available_size = ui.available_size();
                            let max_width = available_size.x * 0.9;
                            let max_height = available_size.y * 0.8;

                            let texture_size = texture.size_vec2();
                            let scale = (max_width / texture_size.x)
                                .min(max_height / texture_size.y)
                                .min(1.0);

                            let display_size = texture_size * scale;
                            ui.image((texture.id(), display_size));
                        } else if self.full_image_loading.is_some() {
                            ui.label(egui::RichText::new("Loading...").size(24.0));
                        } else {
                            // Start loading
                            self.load_full_image(&selected_id, ctx);
                            ui.label(egui::RichText::new("Loading...").size(24.0));
                        }
                    });
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

                            if matches!(self.images, LoadState::NotStarted) {
                                self.load_image_list(ctx);
                            }

                            match self.images.clone() {
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
                                    let thumbnail_size = 250.0;
                                    let spacing = 20.0;
                                    let available_width = ui.available_width();
                                    let cols = ((available_width + spacing) / (thumbnail_size + spacing))
                                        .max(1.0) as usize;

                                    for image in &images {
                                        self.load_thumbnail(&image.id, ctx);
                                    }

                                    egui::Grid::new("image_grid")
                                        .spacing([spacing, spacing])
                                        .show(ui, |ui| {
                                            for (idx, image) in images.iter().enumerate() {
                                                if idx > 0 && idx % cols == 0 {
                                                    ui.end_row();
                                                }

                                                ui.vertical(|ui| {
                                                    let button_response = if let Some(texture) =
                                                        self.thumbnails.get(&image.id)
                                                    {
                                                        let img = egui::Image::new((
                                                            texture.id(),
                                                            Vec2::new(thumbnail_size, thumbnail_size),
                                                        ))
                                                        .sense(egui::Sense::click());
                                                        ui.add(img)
                                                    } else {
                                                        let (rect, response) = ui.allocate_exact_size(
                                                            Vec2::new(thumbnail_size, thumbnail_size),
                                                            egui::Sense::click(),
                                                        );
                                                        ui.painter().rect_filled(
                                                            rect,
                                                            5.0,
                                                            egui::Color32::from_gray(100),
                                                        );
                                                        ui.painter().text(
                                                            rect.center(),
                                                            egui::Align2::CENTER_CENTER,
                                                            "Loading...",
                                                            egui::FontId::proportional(16.0),
                                                            egui::Color32::WHITE,
                                                        );
                                                        response
                                                    };

                                                    if button_response.clicked() {
                                                        self.selected_image = Some(image.id.clone());
                                                    }

                                                    ui.label(
                                                        egui::RichText::new(&image.name).size(14.0),
                                                    );
                                                });
                                            }
                                        });
                                }
                                LoadState::NotStarted => {}
                            }
                        });
                    }
                    Page::Health => {
                        ui.vertical_centered(|ui| {
                            ui.add_space(40.0);

                            ui.heading(egui::RichText::new("Health Check").size(48.0).strong());

                            ui.add_space(30.0);

                            if matches!(self.health, LoadState::NotStarted) {
                                self.load_health(ctx);
                            }

                            match self.health.clone() {
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

fn load_image_from_bytes(bytes: &[u8]) -> Option<ColorImage> {
    match image::load_from_memory(bytes) {
        Ok(dynamic_image) => {
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
