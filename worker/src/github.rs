use common::HistoryData;
use worker::*;

#[cfg(not(use_local_history_file))]
use base64::{engine::general_purpose::STANDARD, Engine};
#[cfg(not(use_local_history_file))]
use serde::{Deserialize, Serialize};

#[cfg(not(use_local_history_file))]
const GITHUB_API_URL: &str =
    "https://api.github.com/repos/aklitzke/family_photos/contents/data/history.toml";

#[cfg(not(use_local_history_file))]
#[derive(Deserialize)]
struct GitHubContentsResponse {
    content: String,
    sha: String,
}

#[cfg(not(use_local_history_file))]
#[derive(Serialize)]
struct GitHubUpdateRequest {
    message: String,
    content: String,
    sha: String,
}

#[cfg(not(use_local_history_file))]
#[derive(Deserialize)]
struct GitHubUpdateResponse {
    _content: GitHubContentInfo,
    commit: GitHubCommitInfo,
}

#[cfg(not(use_local_history_file))]
#[derive(Deserialize)]
struct GitHubContentInfo {
    _sha: String,
}

#[cfg(not(use_local_history_file))]
#[derive(Deserialize)]
struct GitHubCommitInfo {
    sha: String,
}

/// Fetches history.toml from GitHub API (production) or bundled file (local)
/// Returns (HistoryData, SHA) where SHA is used for optimistic locking
pub async fn fetch_history_with_sha(env: &Env) -> Result<(HistoryData, String)> {
    #[cfg(use_local_history_file)]
    {
        let _ = env; // Suppress unused warning in local mode
        console_log!("Local mode: Using bundled history.toml (SHA not available)");
        const HISTORY_TOML_CONTENT: &str = include_str!("../../data/history.toml");
        let data: HistoryData = toml::from_str(HISTORY_TOML_CONTENT)
            .map_err(|e| format!("Failed to parse bundled TOML: {}", e))?;
        return Ok((data, "local-mode-sha".to_string()));
    }

    #[cfg(not(use_local_history_file))]
    {
        console_log!("Fetching history.toml from GitHub API with SHA");
        let mut request = Request::new(GITHUB_API_URL, Method::Get)?;

        let headers = request.headers_mut()?;
        headers.set("User-Agent", "family-photos-worker")?;
        headers.set("Accept", "application/vnd.github.v3+json")?;

        // Add authorization header if GITHUB_TOKEN is available
        if let Ok(token_var) = env.var("GITHUB_TOKEN") {
            let token = token_var.to_string();
            headers.set("Authorization", &format!("Bearer {}", token))?;
        }

        let mut response = Fetch::Request(request).send().await?;

        let status = response.status_code();
        if !(200..300).contains(&status) {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("GitHub API error {}: {}", status, body).into());
        }

        let github_response: GitHubContentsResponse = response.json().await?;
        let sha = github_response.sha.clone();

        // Decode base64 content
        let cleaned_content: String = github_response
            .content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();

        let decoded = STANDARD
            .decode(&cleaned_content)
            .map_err(|e| format!("Failed to decode base64: {}", e))?;

        let toml_content = String::from_utf8(decoded)
            .map_err(|e| format!("Invalid UTF-8 in file content: {}", e))?;

        let data: HistoryData =
            toml::from_str(&toml_content).map_err(|e| format!("Failed to parse TOML: {}", e))?;

        Ok((data, sha))
    }
}

/// Updates history.toml on GitHub with optimistic locking (SHA-based)
/// Returns the new commit SHA on success, or error if SHA doesn't match (409 conflict)
#[cfg(not(use_local_history_file))]
pub async fn update_github_file(
    env: &Env,
    content: &str,
    sha: &str,
    commit_message: &str,
) -> Result<String> {
    console_log!("Updating history.toml on GitHub");

    // Encode content to base64
    let encoded_content = STANDARD.encode(content.as_bytes());

    let github_token = env
        .var("GITHUB_TOKEN")
        .map_err(|_| "GITHUB_TOKEN environment variable not set")?
        .to_string();

    let update_request = GitHubUpdateRequest {
        message: commit_message.to_string(),
        content: encoded_content,
        sha: sha.to_string(),
    };

    let body = serde_json::to_string(&update_request)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;

    let mut request = Request::new_with_init(
        GITHUB_API_URL,
        RequestInit::new()
            .with_method(Method::Put)
            .with_body(Some(wasm_bindgen::JsValue::from_str(&body))),
    )?;

    let headers = request.headers_mut()?;
    headers.set("User-Agent", "family-photos-worker")?;
    headers.set("Accept", "application/vnd.github.v3+json")?;
    headers.set("Authorization", &format!("Bearer {}", github_token))?;
    headers.set("Content-Type", "application/json")?;

    let mut response = Fetch::Request(request).send().await?;

    let status = response.status_code();

    // 409 Conflict means the SHA doesn't match (concurrent update)
    if status == 409 {
        return Err("Conflict: File was modified by another user".into());
    }

    if !(200..300).contains(&status) {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("GitHub API error {}: {}", status, body).into());
    }

    let github_response: GitHubUpdateResponse = response.json().await?;
    Ok(github_response.commit.sha)
}

/// Local mode: Cannot update GitHub file when using bundled history.toml
#[cfg(use_local_history_file)]
pub async fn update_github_file(
    _env: &Env,
    _content: &str,
    _sha: &str,
    commit_message: &str,
) -> Result<String> {
    console_log!("Local mode: Would have committed: {}", commit_message);
    Err("Cannot update GitHub file in local mode (use_local_history_file feature enabled)".into())
}
