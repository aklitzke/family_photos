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

#[derive(Clone, Debug)]
pub struct ArtifactImages {
    pub fronts: Vec<String>,  // fronts[0] = front1, fronts[1] = front2, etc.
    pub backs: Vec<String>,   // backs[0] = back1, backs[1] = back2, etc.
}

impl ArtifactImages {
    /// The primary front image (always present).
    pub fn front1(&self) -> &str {
        &self.fronts[0]
    }

    /// All image keys (fronts then backs).
    pub fn all_keys(&self) -> Vec<&str> {
        self.fronts.iter().map(|s| s.as_str())
            .chain(self.backs.iter().map(|s| s.as_str()))
            .collect()
    }
}

impl Serialize for ArtifactImages {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let count = self.fronts.len() + self.backs.len();
        let mut map = serializer.serialize_map(Some(count))?;
        for (i, front) in self.fronts.iter().enumerate() {
            map.serialize_entry(&format!("front{}", i + 1), front)?;
        }
        for (i, back) in self.backs.iter().enumerate() {
            map.serialize_entry(&format!("back{}", i + 1), back)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ArtifactImages {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map: HashMap<String, String> = HashMap::deserialize(deserializer)?;

        let mut fronts: Vec<(usize, String)> = Vec::new();
        let mut backs: Vec<(usize, String)> = Vec::new();

        for (key, value) in &map {
            if let Some(num_str) = key.strip_prefix("front") {
                if let Ok(num) = num_str.parse::<usize>() {
                    fronts.push((num, value.clone()));
                }
            } else if let Some(num_str) = key.strip_prefix("back") {
                if let Ok(num) = num_str.parse::<usize>() {
                    backs.push((num, value.clone()));
                }
            }
        }

        fronts.sort_by_key(|(n, _)| *n);
        backs.sort_by_key(|(n, _)| *n);

        let fronts: Vec<String> = fronts.into_iter().map(|(_, v)| v).collect();
        let backs: Vec<String> = backs.into_iter().map(|(_, v)| v).collect();

        if fronts.is_empty() {
            return Err(serde::de::Error::custom("ArtifactImages requires at least front1"));
        }

        Ok(ArtifactImages { fronts, backs })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArtifactUpdate {
    pub author: String,
    pub updated: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub people: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
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

/// Returns the most recent tags from an artifact's update history.
pub fn artifact_tags(artifact: &Artifact) -> &[String] {
    artifact.updates.iter().rev()
        .find_map(|u| u.tags.as_deref())
        .unwrap_or(&[])
}

/// Returns the most recent people from an artifact's update history.
pub fn artifact_people(artifact: &Artifact) -> &[String] {
    artifact.updates.iter().rev()
        .find_map(|u| u.people.as_deref())
        .unwrap_or(&[])
}

/// Returns the most recent location from an artifact's update history.
pub fn artifact_location(artifact: &Artifact) -> Option<&str> {
    artifact.updates.iter().rev()
        .find_map(|u| u.location.as_deref())
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
    pub tags: Option<Vec<String>>,
    pub people: Option<Vec<String>>,
    pub location: Option<String>,
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

#[derive(Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LoginResponse {
    pub success: bool,
    pub username: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MeResponse {
    pub username: String,
}

#[derive(Serialize, Deserialize)]
pub struct MergeArtifactsRequest {
    pub leader_id: u32,
    pub follower_id: u32,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MergeArtifactsResponse {
    pub success: bool,
    pub merged_artifact: Artifact,
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
