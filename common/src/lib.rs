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

#[derive(Serialize, Deserialize)]
pub struct HistoryData {
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
