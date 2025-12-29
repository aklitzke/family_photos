use aws_config::BehaviorVersion;
use aws_sdk_s3::config::{Credentials, Region};
use aws_sdk_s3::Client;
use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fs;

#[derive(Deserialize)]
struct WranglerConfig {
    r2_buckets: Vec<R2Bucket>,
}

#[derive(Deserialize)]
struct R2Bucket {
    binding: String,
    bucket_name: String,
}

pub struct R2File {
    pub key: String,
    pub size: i64,
}

pub async fn list_r2_bucket(bucket_binding: &str) -> Result<(String, Vec<R2File>), Box<dyn Error>> {
    // Load .env file if it exists
    dotenv::dotenv().ok();

    // Read wrangler.toml to get bucket name
    let wrangler_path = "../worker/wrangler.toml";
    let wrangler_content = fs::read_to_string(wrangler_path)?;
    let config: WranglerConfig = toml::from_str(&wrangler_content)?;

    // Find the bucket
    let bucket = config
        .r2_buckets
        .iter()
        .find(|b| b.binding == bucket_binding)
        .ok_or(format!("{} bucket not found in wrangler.toml", bucket_binding))?;

    let bucket_name = bucket.bucket_name.clone();

    // Get R2 credentials from environment
    let account_id = env::var("CLOUDFLARE_ACCOUNT_ID")?;
    let access_key_id = env::var("R2_ACCESS_KEY_ID")?;
    let secret_access_key = env::var("R2_SECRET_ACCESS_KEY")?;

    // Configure S3 client for R2
    let endpoint = format!("https://{}.r2.cloudflarestorage.com", account_id);
    let credentials = Credentials::new(
        access_key_id,
        secret_access_key,
        None,
        None,
        "r2-credentials",
    );

    let region = Region::new("auto");
    let config = aws_sdk_s3::Config::builder()
        .endpoint_url(endpoint)
        .credentials_provider(credentials)
        .region(region)
        .behavior_version(BehaviorVersion::latest())
        .force_path_style(true)
        .build();

    let client = Client::from_conf(config);

    // List objects in the bucket
    let mut files = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let mut request = client.list_objects_v2().bucket(&bucket_name);

        if let Some(token) = continuation_token {
            request = request.continuation_token(token);
        }

        let response = request.send().await?;

        for object in response.contents() {
            if let Some(key) = object.key() {
                let size = object.size().unwrap_or(0);
                files.push(R2File {
                    key: key.to_string(),
                    size,
                });
            }
        }

        if response.is_truncated() == Some(true) {
            continuation_token = response.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }

    Ok((bucket_name, files))
}
