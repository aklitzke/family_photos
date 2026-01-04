use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
pub struct Artifact {
    pub images: ArtifactImages,
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
