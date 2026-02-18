use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use toml_edit::DocumentMut;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImageMetadata {
    pub key: String,
    pub rotation: Option<u16>,
}

#[derive(Serialize, Deserialize)]
pub struct ImageListResponse {
    pub images: Vec<ImageMetadata>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HealthResponse {
    pub status: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArtifactImages {
    pub front1: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front2: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub back1: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArtifactUpdate {
    pub author: String,
    pub updated: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Artifact {
    pub id: u32,
    pub images: ArtifactImages,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updates: Vec<ArtifactUpdate>,
}

/// Returns the most recent date from an artifact's update history.
pub fn artifact_date(artifact: &Artifact) -> Option<&str> {
    artifact.updates.iter().rev()
        .find_map(|u| u.date.as_deref())
}

#[derive(Serialize, Deserialize)]
pub struct ArtifactListResponse {
    pub artifacts: Vec<Artifact>,
}

#[derive(Serialize, Deserialize)]
pub struct HistoryData {
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    pub images: Vec<ImageMetadata>,
}

#[derive(Serialize, Deserialize)]
pub struct ThumbnailBatchRequest {
    pub keys: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ThumbnailBatchResponse {
    pub thumbnails: HashMap<String, Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
pub struct PresignedUrlResponse {
    pub url: String,
}

#[derive(Serialize, Deserialize)]
pub struct RotateImageRequest {
    pub image_key: String,
    pub new_rotation: u16,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RotateImageResponse {
    pub success: bool,
    pub old_rotation: Option<u16>,
    pub new_rotation: u16,
}

#[derive(Serialize, Deserialize)]
pub struct UpdateArtifactRequest {
    pub artifact_id: u32,
    pub reason: String,
    pub date: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UpdateArtifactResponse {
    pub success: bool,
    pub update: ArtifactUpdate,
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub error_type: String,
}

/// Formats HistoryData to TOML string with proper formatting
/// - Uses dotted key notation for artifact images (e.g., images.front1 = "...")
/// - Preserves consistent formatting across all tools
pub fn format_history_toml(data: &HistoryData) -> Result<String, String> {
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
