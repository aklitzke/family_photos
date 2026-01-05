mod github;
mod sigv4;

use common::{
    ArtifactListResponse, ErrorResponse, HealthResponse, HistoryData, ImageListResponse,
    ImageMetadata, PresignedUrlResponse, RotateImageRequest, RotateImageResponse,
    ThumbnailBatchRequest, ThumbnailBatchResponse,
};
use github::{fetch_history_with_sha, update_github_file};
use sigv4::generate_r2_presigned_url;
use std::collections::HashMap;
use worker::*;

#[cfg(not(use_local_history_file))]
const MAX_RETRIES: u32 = 3;

#[cfg(not(use_local_history_file))]
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

#[cfg(use_local_history_file)]
async fn fetch_images_with_retry(env: &Env) -> Result<Vec<ImageMetadata>> {
    fetch_images(env).await
}

async fn fetch_history_data(env: &Env) -> Result<HistoryData> {
    // Use the github module function and ignore the SHA
    let (data, _sha) = fetch_history_with_sha(env).await?;
    Ok(data)
}

async fn fetch_images(env: &Env) -> Result<Vec<ImageMetadata>> {
    let data = fetch_history_data(env).await?;
    Ok(data.images)
}

/// Formats HistoryData to TOML string with proper formatting
/// - Uses dotted key notation for artifact images (e.g., images.front1 = "...")
/// - Preserves consistent formatting across all tools
pub fn format_history_toml(data: &HistoryData) -> Result<String, String> {
    use toml_edit::DocumentMut;

    // First serialize to TOML string, then parse with toml_edit for formatting control
    let toml_string = toml::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize to TOML: {}", e))?;

    let mut doc = toml_string.parse::<DocumentMut>()
        .map_err(|e| format!("Failed to parse TOML document: {}", e))?;

    // Convert artifacts array to use dotted keys for images
    if let Some(artifacts_array) = doc.get_mut("artifacts").and_then(|item| item.as_array_of_tables_mut()) {
        for artifact in artifacts_array.iter_mut() {
            // Mark the images table as dotted to use dotted key notation
            if let Some(images) = artifact.get_mut("images") {
                if let Some(images_table) = images.as_table_like_mut() {
                    images_table.set_dotted(true);
                }
            }
        }
    }

    Ok(doc.to_string())
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
        .options("/api/images/rotate", |_req, _ctx| {
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
        .get_async("/api/artifacts/list", |_req, ctx| async move {
            let history_data = fetch_history_data(&ctx.env).await?;
            Response::from_json(&ArtifactListResponse { artifacts: history_data.artifacts })
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
        .post_async("/api/images/rotate", |mut req, ctx| async move {
            // Parse request body
            let request: RotateImageRequest = match req.json().await {
                Ok(r) => r,
                Err(e) => {
                    return Response::from_json(&ErrorResponse {
                        error: format!("Invalid request body: {}", e),
                        error_type: "validation".to_string(),
                    })
                    .map(|r| r.with_status(400));
                }
            };

            // Validate rotation value
            if ![0, 90, 180, 270].contains(&request.new_rotation) {
                return Response::from_json(&ErrorResponse {
                    error: "Invalid rotation value. Must be 0, 90, 180, or 270".to_string(),
                    error_type: "validation".to_string(),
                })
                .map(|r| r.with_status(400));
            }

            // Fetch current history with SHA
            let (mut history_data, sha) = match fetch_history_with_sha(&ctx.env).await {
                Ok(data) => data,
                Err(e) => {
                    return Response::from_json(&ErrorResponse {
                        error: format!("Failed to fetch history data: {}", e),
                        error_type: "github_api".to_string(),
                    })
                    .map(|r| r.with_status(500));
                }
            };

            // Find the image and update rotation
            let mut found = false;
            let mut old_rotation = None;

            // TODO convert to iterator
            // TODO only do a write if the rotation has changed
            for image in &mut history_data.images {
                if image.key == request.image_key {
                    found = true;
                    old_rotation = image.rotation;

                    // Update rotation (set to None if 0, otherwise set value)
                    image.rotation = if request.new_rotation == 0 {
                        None
                    } else {
                        Some(request.new_rotation)
                    };
                    break;
                }
            }

            if !found {
                return Response::from_json(&ErrorResponse {
                    error: format!("Image not found: {}", request.image_key),
                    error_type: "validation".to_string(),
                })
                .map(|r| r.with_status(404));
            }

            // Serialize updated history to TOML with proper formatting
            let toml_content = match format_history_toml(&history_data) {
                Ok(content) => content,
                Err(e) => {
                    return Response::from_json(&ErrorResponse {
                        error: format!("Failed to serialize TOML: {}", e),
                        error_type: "internal".to_string(),
                    })
                    .map(|r| r.with_status(500));
                }
            };

            // Create commit message
            let old_rotation_str = old_rotation
                .map(|r| format!("{}°", r))
                .unwrap_or_else(|| "0°".to_string());
            let new_rotation_str = if request.new_rotation == 0 {
                "0°".to_string()
            } else {
                format!("{}°", request.new_rotation)
            };

            let commit_message = format!(
                "Autoupdate: <user> Updated {} rotation: {} → {}",
                request.image_key, old_rotation_str, new_rotation_str
            );

            // Update GitHub file with optimistic locking
            match update_github_file(&ctx.env, &toml_content, &sha, &commit_message).await {
                Ok(_commit_sha) => {
                    console_log!("Successfully updated rotation for {}", request.image_key);
                    Response::from_json(&RotateImageResponse {
                        success: true,
                        old_rotation,
                        new_rotation: request.new_rotation,
                    })
                }
                Err(e) if e.to_string().contains("Conflict") => Response::from_json(&ErrorResponse {
                    error: "The file was modified by another user. Please refresh and try again."
                        .to_string(),
                    error_type: "conflict".to_string(),
                })
                .map(|r| r.with_status(409)),
                Err(e) => Response::from_json(&ErrorResponse {
                    error: format!("Failed to update GitHub: {}", e),
                    error_type: "github_api".to_string(),
                })
                .map(|r| r.with_status(500)),
            }
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
