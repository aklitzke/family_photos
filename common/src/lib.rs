use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImageMetadata {
    pub id: String,
    pub key: String,
    pub name: String,
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
