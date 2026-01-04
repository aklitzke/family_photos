use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use worker::*;

/// Generate an AWS SigV4 presigned URL for Cloudflare R2
pub fn generate_r2_presigned_url(
    account_id: &str,
    access_key_id: &str,
    secret_access_key: &str,
    object_key: &str,
    expires_in_seconds: u64,
) -> Result<String> {
    // Get current timestamp
    let now_millis = Date::now().as_millis();
    let now_secs = now_millis / 1000;
    let date_time = format_timestamp(now_secs);
    let date = &date_time[0..8]; // YYYYMMDD

    // Constants
    let region = "auto";
    let service = "s3";
    let host = format!("{}.r2.cloudflarestorage.com", account_id);
    let bucket = "google-drive-pics";

    // URL-encode the object key path segments
    let encoded_key = object_key
        .split('/')
        .map(|segment| urlencoding::encode(segment).into_owned())
        .collect::<Vec<_>>()
        .join("/");

    let canonical_uri = format!("/{}/{}", bucket, encoded_key);

    // Build credential scope
    let credential_scope = format!("{}/{}/{}/aws4_request", date, region, service);
    let credential = format!("{}/{}", access_key_id, credential_scope);

    // Build canonical query string (must be sorted)
    let mut query_params = vec![
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
        ("X-Amz-Credential", urlencoding::encode(&credential).into_owned()),
        ("X-Amz-Date", date_time.clone()),
        ("X-Amz-Expires", expires_in_seconds.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
    ];
    query_params.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_query_string = query_params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");

    // Build canonical request
    let canonical_headers = format!("host:{}\n", host);
    let signed_headers = "host";
    let payload_hash = "UNSIGNED-PAYLOAD";

    let canonical_request = format!(
        "GET\n{}\n{}\n{}\n{}\n{}",
        canonical_uri,
        canonical_query_string,
        canonical_headers,
        signed_headers,
        payload_hash
    );

    // Hash canonical request
    let mut hasher = Sha256::new();
    hasher.update(canonical_request.as_bytes());
    let canonical_request_hash = hex::encode(hasher.finalize());

    // Build string to sign
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        date_time,
        credential_scope,
        canonical_request_hash
    );

    // Calculate signing key
    let k_secret = format!("AWS4{}", secret_access_key);
    let k_date = hmac_sha256(k_secret.as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let signing_key = hmac_sha256(&k_service, b"aws4_request");

    // Calculate signature
    let signature = hmac_sha256(&signing_key, string_to_sign.as_bytes());
    let signature_hex = hex::encode(signature);

    // Build final presigned URL
    let presigned_url = format!(
        "https://{}{}?{}&X-Amz-Signature={}",
        host,
        canonical_uri,
        canonical_query_string,
        signature_hex
    );

    Ok(presigned_url)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn format_timestamp(unix_secs: u64) -> String {
    // Convert Unix timestamp to YYYYMMDDTHHMMSSZ format
    let secs_per_day = 86400;
    let days_since_epoch = unix_secs / secs_per_day;
    let secs_today = unix_secs % secs_per_day;

    // Calculate date (simplified - doesn't account for leap years perfectly but close enough)
    let mut year = 1970;
    let mut remaining_days = days_since_epoch;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    // Calculate month and day
    let days_in_months = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    let mut day = remaining_days + 1;

    for &days_in_month in &days_in_months {
        if day <= days_in_month {
            break;
        }
        day -= days_in_month;
        month += 1;
    }

    // Calculate time
    let hours = secs_today / 3600;
    let minutes = (secs_today % 3600) / 60;
    let seconds = secs_today % 60;

    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn is_leap_year(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
