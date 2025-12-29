use base64::{engine::general_purpose::STANDARD, Engine};
use common::{HealthResponse, HistoryData, ImageListResponse, ImageMetadata};
use serde::Deserialize;
use worker::*;

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/aklitzke/family_photos/contents/data/history.toml";
const MAX_RETRIES: u32 = 3;

#[derive(Deserialize)]
struct GitHubContentsResponse {
    content: String,
    #[allow(dead_code)] // Will be used for future write operations
    sha: String,
}

async fn fetch_images_with_retry() -> Result<Vec<ImageMetadata>> {
    for attempt in 1..=MAX_RETRIES {
        match fetch_images().await {
            Ok(images) => return Ok(images),
            Err(e) if attempt < MAX_RETRIES => {
                console_log!("Fetch attempt {} failed: {}, retrying...", attempt, e);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

async fn fetch_images() -> Result<Vec<ImageMetadata>> {
    let mut request = Request::new(GITHUB_API_URL, Method::Get)?;

    // GitHub API requires User-Agent header
    let headers = request.headers_mut()?;
    headers.set("User-Agent", "family-photos-worker")?;
    headers.set("Accept", "application/vnd.github.v3+json")?;

    let mut response = Fetch::Request(request).send().await?;

    let status = response.status_code();
    if !(200..300).contains(&status) {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("GitHub API error {}: {}", status, body).into());
    }

    let github_response: GitHubContentsResponse = response.json().await?;

    // Decode base64 content (GitHub returns it with newlines, so remove whitespace first)
    let cleaned_content: String = github_response
        .content
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    let decoded = STANDARD
        .decode(&cleaned_content)
        .map_err(|e| format!("Failed to decode base64: {}", e))?;

    let toml_content =
        String::from_utf8(decoded).map_err(|e| format!("Invalid UTF-8 in file content: {}", e))?;

    // Parse TOML
    let data: HistoryData =
        toml::from_str(&toml_content).map_err(|e| format!("Failed to parse TOML: {}", e))?;

    Ok(data.images)
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
            let images = fetch_images_with_retry().await?;
            Response::from_json(&ImageListResponse { images })
        })
        .get_async("/api/images/thumbnail/:id", |_req, ctx| async move {
            let id = ctx.param("id").ok_or("Missing id parameter")?;

            // Fetch images and find the image key from the ID
            let image_entries = fetch_images_with_retry().await?;
            let image_key = image_entries
                .iter()
                .find(|entry| entry.id == *id)
                .map(|entry| entry.key.as_str())
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

            // Fetch images and find the image key from the ID
            let image_entries = fetch_images_with_retry().await?;
            let image_key = image_entries
                .iter()
                .find(|entry| entry.id == *id)
                .map(|entry| entry.key.as_str())
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
