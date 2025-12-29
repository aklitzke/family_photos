use serde::{Deserialize, Serialize};
use worker::*;

// Hardcoded list of 5 images - UPDATE THESE TO MATCH YOUR R2 OBJECTS
const IMAGES: &[(&str, &str, &str)] = &[
    ("1", "Family History/Pile 1/2025-12-23-21-51-0001.jpg", "Family Photo 1"),
    ("2", "Family History/Pile 1/2025-12-23-21-51-0002.jpg", "Family Photo 2"),
    ("3", "Family History/Pile 1/2025-12-23-21-51-0003.jpg", "Family Photo 3"),
    ("4", "Family History/Pile 1/2025-12-23-21-51-0004.jpg", "Family Photo 4"),
    ("5", "Family History/Pile 1/2025-12-23-21-54-0001.jpg", "Family Photo 5"),
];

#[derive(Serialize, Deserialize)]
struct ImageMetadata {
    id: String,
    key: String,
    name: String,
}

#[derive(Serialize)]
struct ImageListResponse {
    images: Vec<ImageMetadata>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    message: String,
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Set up CORS
    let cors = Cors::new()
        .with_origins(vec!["*"])
        .with_methods(vec![Method::Get, Method::Post, Method::Options])
        .with_allowed_headers(vec!["Content-Type"]);

    let router = Router::with_data(&env);

    router
        .get_async("/api/health", |_req, _ctx| async move {
            Response::from_json(&HealthResponse {
                status: "ok".to_string(),
                message: "Worker is running".to_string(),
            })
        })
        .get_async("/api/images/list", |_req, _ctx| async move {
            let images: Vec<ImageMetadata> = IMAGES
                .iter()
                .map(|(id, key, name)| ImageMetadata {
                    id: id.to_string(),
                    key: key.to_string(),
                    name: name.to_string(),
                })
                .collect();

            Response::from_json(&ImageListResponse { images })
        })
        .get_async("/api/images/thumbnail/:id", |_req, ctx| async move {
            let id = ctx.param("id").ok_or("Missing id parameter")?;

            // Find the image key from the ID
            let image_key = IMAGES
                .iter()
                .find(|(img_id, _, _)| *img_id == id)
                .map(|(_, key, _)| *key)
                .ok_or("Image not found")?;

            // Prefix thumbnail key with source bucket binding name
            let thumbnail_key = format!("google_drive_pics/{}", image_key);

            // Get R2 buckets
            let source_bucket = ctx.env.bucket("google_drive_pics")?;
            let thumbnails_bucket = ctx.env.bucket("thumbnails")?;

            // Check if thumbnail exists in thumbnails bucket
            match thumbnails_bucket.get(&thumbnail_key).execute().await {
                Ok(Some(object)) => {
                    // Thumbnail exists, return it
                    console_log!("Thumbnail exists in R2: {}", thumbnail_key);
                    let body = object.body().ok_or("No body")?;
                    return Response::from_bytes(body.bytes().await?)
                        .map(|r| r.with_headers(Headers::from_iter(vec![
                            ("Content-Type", "image/jpeg"),
                            ("Cache-Control", "public, max-age=31536000"),
                        ])));
                }
                _ => {
                    // Thumbnail doesn't exist, generate it
                    console_log!("Generating new thumbnail for: {}", image_key);
                }
            }

            // Fetch original image from source bucket
            let original_object = source_bucket
                .get(image_key)
                .execute()
                .await?
                .ok_or("Original image not found")?;

            let original_body = original_object.body().ok_or("No body")?;
            let image_bytes = original_body.bytes().await?;

            // Generate thumbnail
            let thumbnail_bytes = generate_thumbnail(&image_bytes, 300)
                .map_err(|e| format!("Failed to generate thumbnail: {}", e))?;

            // Upload thumbnail to thumbnails bucket
            thumbnails_bucket
                .put(&thumbnail_key, thumbnail_bytes.clone())
                .execute()
                .await?;

            console_log!("Uploaded new thumbnail to R2: {}", thumbnail_key);

            Response::from_bytes(thumbnail_bytes).map(|r| {
                r.with_headers(Headers::from_iter(vec![
                    ("Content-Type", "image/jpeg"),
                    ("Cache-Control", "public, max-age=31536000"),
                ]))
            })
        })
        .get_async("/api/images/full/:id", |_req, ctx| async move {
            let id = ctx.param("id").ok_or("Missing id parameter")?;

            // Find the image key from the ID
            let image_key = IMAGES
                .iter()
                .find(|(img_id, _, _)| *img_id == id)
                .map(|(_, key, _)| *key)
                .ok_or("Image not found")?;

            // Get source bucket
            let source_bucket = ctx.env.bucket("google_drive_pics")?;

            // Fetch image
            let object = source_bucket
                .get(image_key)
                .execute()
                .await?
                .ok_or("Image not found")?;

            let body = object.body().ok_or("No body")?;
            let image_bytes = body.bytes().await?;

            Response::from_bytes(image_bytes).map(|r| {
                r.with_headers(Headers::from_iter(vec![
                    ("Content-Type", "image/jpeg"),
                    ("Cache-Control", "public, max-age=86400"),
                ]))
            })
        })
        .run(req, env.clone())
        .await?
        .with_cors(&cors)
}

fn generate_thumbnail(image_data: &[u8], width: u32) -> Result<Vec<u8>> {
    use image::{imageops::FilterType, ImageFormat};
    use std::io::Cursor;

    // Load image
    let img = image::load_from_memory(image_data)
        .map_err(|e| format!("Failed to load image: {}", e))?;

    // Calculate height maintaining aspect ratio
    let height = (width as f32 * img.height() as f32 / img.width() as f32) as u32;

    // Resize image
    let thumbnail = img.resize(width, height, FilterType::Lanczos3);

    // Convert to JPEG
    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);
    thumbnail
        .write_to(&mut cursor, ImageFormat::Jpeg)
        .map_err(|e| format!("Failed to encode JPEG: {}", e))?;

    Ok(output)
}
