use eframe::egui;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen]
pub fn start() -> Result<(), JsValue> {
    // Make sure panics are logged to the console
    console_error_panic_hook::set_once();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        // Get the canvas element
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
            Ok(_) => {},
            Err(e) => {
                web_sys::console::error_1(&format!("Failed to start eframe: {e:?}").into());
            }
        }
    });

    Ok(())
}

struct FamilyPhotosApp {
    health_status: Option<String>,
}

impl FamilyPhotosApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            health_status: None,
        }
    }

    fn check_health(&mut self) {
        wasm_bindgen_futures::spawn_local(async move {
            match fetch_health().await {
                Ok(status) => {
                    web_sys::console::log_1(&format!("Health check: {}", status).into());
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("Health check failed: {}", e).into());
                }
            }
        });
    }
}

impl eframe::App for FamilyPhotosApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(60.0);

                // Title
                ui.heading(egui::RichText::new("Family Photos")
                    .size(48.0)
                    .strong());

                ui.add_space(20.0);

                // Subtitle
                ui.label(egui::RichText::new("Preserve your precious memories in one beautiful place")
                    .size(20.0));

                ui.add_space(10.0);

                // Description
                ui.label(egui::RichText::new(
                    "A simple, elegant way to store, organize, and share your family's most cherished moments.\n\
                     Built with modern technology to keep your memories safe and accessible.")
                    .size(14.0));

                ui.add_space(30.0);

                // Buttons
                ui.horizontal(|ui| {
                    if ui.button(egui::RichText::new("Get Started").size(16.0)).clicked() {
                        web_sys::console::log_1(&"Get Started clicked!".into());
                    }

                    if ui.button(egui::RichText::new("Learn More").size(16.0)).clicked() {
                        web_sys::console::log_1(&"Learn More clicked!".into());
                    }

                    if ui.button(egui::RichText::new("Check Health").size(16.0)).clicked() {
                        self.check_health();
                    }
                });

                ui.add_space(40.0);

                // Features section
                ui.separator();
                ui.add_space(30.0);

                ui.heading(egui::RichText::new("Features").size(32.0));
                ui.add_space(20.0);

                ui.columns(2, |columns| {
                    columns[0].vertical_centered(|ui| {
                        ui.label(egui::RichText::new("📸").size(40.0));
                        ui.heading("Easy Upload");
                        ui.label("Upload your photos with a simple drag and drop interface");
                    });

                    columns[1].vertical_centered(|ui| {
                        ui.label(egui::RichText::new("🗂️").size(40.0));
                        ui.heading("Smart Organization");
                        ui.label("Automatically organize photos by date, event, or custom albums");
                    });
                });

                ui.add_space(30.0);

                ui.columns(2, |columns| {
                    columns[0].vertical_centered(|ui| {
                        ui.label(egui::RichText::new("🔒").size(40.0));
                        ui.heading("Secure Storage");
                        ui.label("Your memories are safely stored and backed up");
                    });

                    columns[1].vertical_centered(|ui| {
                        ui.label(egui::RichText::new("👨‍👩‍👧‍👦").size(40.0));
                        ui.heading("Family Sharing");
                        ui.label("Share albums with family members effortlessly");
                    });
                });

                ui.add_space(40.0);
                ui.separator();
                ui.add_space(20.0);

                // Footer
                ui.label(egui::RichText::new("© 2025 Family Photos. Built with Rust & egui.")
                    .size(12.0)
                    .weak());
            });
        });
    }
}

async fn fetch_health() -> Result<String, String> {
    use web_sys::{Request, RequestInit, RequestMode, Response};

    let mut opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);

    let request = Request::new_with_str_and_init("/api/health", &opts)
        .map_err(|e| format!("Failed to create request: {:?}", e))?;

    let window = web_sys::window().unwrap();
    let resp_value = wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("Fetch failed: {:?}", e))?;

    let resp: Response = resp_value.dyn_into().unwrap();
    let json = wasm_bindgen_futures::JsFuture::from(resp.json().unwrap())
        .await
        .map_err(|e| format!("Failed to parse JSON: {:?}", e))?;

    Ok(format!("{:?}", json))
}
