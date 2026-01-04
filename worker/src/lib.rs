mod sigv4;

use common::{HealthResponse, HistoryData, ImageListResponse, ImageMetadata, PresignedUrlResponse, ThumbnailBatchRequest, ThumbnailBatchResponse};
use sigv4::generate_r2_presigned_url;
use std::collections::HashMap;
use worker::*;

#[cfg(not(debug_assertions))]
use base64::{engine::general_purpose::STANDARD, Engine};
#[cfg(not(debug_assertions))]
use serde::Deserialize;

#[cfg(not(debug_assertions))]
const GITHUB_API_URL: &str =
    "https://api.github.com/repos/aklitzke/family_photos/contents/data/history.toml";
#[cfg(not(debug_assertions))]
const MAX_RETRIES: u32 = 3;

#[cfg(not(debug_assertions))]
#[derive(Deserialize)]
struct GitHubContentsResponse {
    content: String,
    #[allow(dead_code)] // Will be used for future write operations
    sha: String,
}

#[cfg(not(debug_assertions))]
async fn fetch_images_with_retry(env: &Env) -> Result<Vec<ImageMetadata>> {
    for attempt in 1..=MAX_RETRIES {
        match fetch_images(env).await {
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

#[cfg(debug_assertions)]
async fn fetch_images_with_retry(env: &Env) -> Result<Vec<ImageMetadata>> {
    fetch_images(env).await
}

async fn fetch_images(_env: &Env) -> Result<Vec<ImageMetadata>> {
    // In debug builds (local dev), use bundled history.toml
    #[cfg(debug_assertions)]
    {
        console_log!("Using bundled history.toml from local file (debug mode)");
        const HISTORY_TOML_CONTENT: &str = include_str!("../../data/history.toml");
        let data: HistoryData = toml::from_str(HISTORY_TOML_CONTENT)
            .map_err(|e| format!("Failed to parse bundled TOML: {}", e))?;
        return Ok(data.images);
    }

    // In release builds (production), fetch from GitHub
    #[cfg(not(debug_assertions))]
    {
        console_log!("Fetching history.toml from GitHub API");
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
        .options("/api/images/thumbnails", |_req, _ctx| {
            Response::empty()
        })
        .get_async("/api/health", |_req, _ctx| async move {
            Response::from_json(&HealthResponse {
                status: "ok".to_string(),
                message: "Worker is running".to_string(),
            })
        })
        .get_async("/api/images/list", |_req, ctx| async move {
            let images = fetch_images_with_retry(&ctx.env).await?;
            Response::from_json(&ImageListResponse { images })
        })
        .get_async("/api/images/thumbnail", |req, ctx| async move {
            let url = req.url()?;
            let image_key = url
                .query_pairs()
                .find(|(k, _)| k == "key")
                .map(|(_, v)| v.to_string())
                .ok_or("Missing key parameter")?;

            // Change extension to .jpg for thumbnail lookup (all thumbnails are JPEG)
            use std::path::Path;
            let image_path = Path::new(&image_key);
            let thumbnail_key_base = image_path.with_extension("jpg");
            let thumbnail_key_str = thumbnail_key_base.to_str().ok_or("Invalid path")?;

            // Prefix thumbnail key with source bucket binding name
            let thumbnail_key = format!("google_drive_pics/{}", thumbnail_key_str);

            // Get R2 buckets
            let source_bucket = ctx.env.bucket("google_drive_pics")?;
            let thumbnails_bucket = ctx.env.bucket("thumbnails")?;

            // Check if thumbnail exists in thumbnails bucket
            match thumbnails_bucket.get(&thumbnail_key).execute().await {
                Ok(Some(object)) => {
                    // Thumbnail exists, return it
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
        .post_async("/api/images/thumbnails", |mut req, ctx| async move {
            // Parse request body
            let request: ThumbnailBatchRequest = req.json().await?;

            // Get R2 buckets
            let source_bucket = ctx.env.bucket("google_drive_pics")?;
            let thumbnails_bucket = ctx.env.bucket("thumbnails")?;

            let mut thumbnails = HashMap::new();

            // Process each requested key
            for image_key in request.keys {
                // Change extension to .jpg for thumbnail lookup (all thumbnails are JPEG)
                use std::path::Path;
                let image_path = Path::new(&image_key);
                let thumbnail_key_base = image_path.with_extension("jpg");
                let thumbnail_key_str = thumbnail_key_base.to_str().ok_or("Invalid path")?;
                let thumbnail_key = format!("google_drive_pics/{}", thumbnail_key_str);

                // Try to get existing thumbnail
                let thumbnail_bytes = match thumbnails_bucket.get(&thumbnail_key).execute().await {
                    Ok(Some(object)) => {
                        let body = object.body().ok_or("No body")?;
                        body.bytes().await?
                    }
                    _ => {
                        // Generate thumbnail
                        console_log!("Generating new thumbnail for: {}", image_key);

                        // Fetch original image
                        let original_object = match source_bucket.get(image_key.as_str()).execute().await {
                            Ok(Some(obj)) => obj,
                            _ => {
                                console_log!("Failed to fetch original image: {}", image_key);
                                continue; // Skip if original not found
                            }
                        };

                        let original_body = original_object.body().ok_or("No body")?;
                        let image_bytes = original_body.bytes().await?;

                        // Generate thumbnail
                        let thumbnail = match generate_thumbnail(&image_bytes, 300) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                console_log!("Failed to generate thumbnail for {}: {}", image_key, e);
                                continue; // Skip on error
                            }
                        };

                        // Upload thumbnail to R2
                        if let Err(e) = thumbnails_bucket.put(&thumbnail_key, thumbnail.clone()).execute().await {
                            console_log!("Failed to upload thumbnail for {}: {:?}", image_key, e);
                        } else {
                            console_log!("Uploaded new thumbnail to R2: {}", thumbnail_key);
                        }

                        thumbnail
                    }
                };

                thumbnails.insert(image_key, thumbnail_bytes);
            }

            Response::from_json(&ThumbnailBatchResponse { thumbnails })
        })
        .get_async("/api/images/full", |req, ctx| async move {
            let url = req.url()?;
            let image_key = url
                .query_pairs()
                .find(|(k, _)| k == "key")
                .map(|(_, v)| v.to_string())
                .ok_or("Missing key parameter")?;

            // Get R2 credentials from environment
            let account_id = ctx.env.var("CLOUDFLARE_ACCOUNT_ID")?.to_string();
            let access_key_id = ctx.env.var("R2_ACCESS_KEY_ID")?.to_string();
            let secret_access_key = ctx.env.var("R2_SECRET_ACCESS_KEY")?.to_string();

            // Generate presigned URL manually using SigV4
            let presigned_url = generate_r2_presigned_url(
                &account_id,
                &access_key_id,
                &secret_access_key,
                &image_key,
                3600,
            )?;

            Response::from_json(&PresignedUrlResponse { url: presigned_url })
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
