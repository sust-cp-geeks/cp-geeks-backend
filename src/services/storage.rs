use serde::Deserialize;

use crate::errors::AppError;
use crate::services::http;

// how long an admin's view link stays valid
const VIEW_URL_TTL_SECS: u64 = 300;

// supabase settings, read at call time so the server still boots (and the
// sust-email signup path still works) when storage isn't configured yet
struct Config {
    base: String,
    service_key: String,
    bucket: String,
}

fn config() -> Result<Config, AppError> {
    Ok(Config {
        // trailing slash is easy to paste in by accident
        base: env("SUPABASE_URL")?.trim_end_matches('/').to_string(),
        service_key: secret_key()?,
        bucket: env("SUPABASE_BUCKET")?,
    })
}

// supabase now issues `sb_secret_...` keys and calls the old JWT ones legacy
// service_role, so accept either name rather than caring which era you're in
fn secret_key() -> Result<String, AppError> {
    for key in ["SUPABASE_SECRET_KEY", "SUPABASE_SERVICE_KEY"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return Ok(value.trim().to_string());
            }
        }
    }
    tracing::error!("no supabase secret key set — ID card storage is unavailable");
    Err(AppError::InternalError(
        "File storage is not configured".to_string(),
    ))
}

fn env(key: &str) -> Result<String, AppError> {
    std::env::var(key).map_err(|_| {
        tracing::error!("{} is not set — ID card storage is unavailable", key);
        AppError::InternalError("File storage is not configured".to_string())
    })
}

// returns true when storage is fully configured, so callers can fail early with
// a clear message instead of part-way through a signup
pub fn is_configured() -> bool {
    let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.trim().is_empty());
    set("SUPABASE_URL")
        && set("SUPABASE_BUCKET")
        && (set("SUPABASE_SECRET_KEY") || set("SUPABASE_SERVICE_KEY"))
}

// uploads bytes under `key`, replacing anything already there
pub async fn upload(key: &str, body: Vec<u8>, content_type: &str) -> Result<(), AppError> {
    let cfg = config()?;
    let url = format!("{}/storage/v1/object/{}/{}", cfg.base, cfg.bucket, key);

    let response = http::storage()
        .post(&url)
        .bearer_auth(&cfg.service_key)
        .header("apikey", &cfg.service_key)
        .header("content-type", content_type)
        // without this a retry of the same key comes back as 409 Duplicate
        .header("x-upsert", "true")
        .body(body)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("storage upload failed for {}: {}", key, e);
            AppError::InternalError("Failed to store file".to_string())
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        tracing::error!("storage upload rejected for {}: {} {}", key, status, detail);
        return Err(AppError::InternalError("Failed to store file".to_string()));
    }

    tracing::info!("stored {}", key);
    Ok(())
}

#[derive(Deserialize)]
struct SignedUrlResponse {
    #[serde(rename = "signedURL")]
    signed_url: String,
}

// short-lived link so an admin can view a card without the bucket being public
pub async fn view_url(key: &str) -> Result<String, AppError> {
    let cfg = config()?;
    let url = format!("{}/storage/v1/object/sign/{}/{}", cfg.base, cfg.bucket, key);

    let response = http::storage()
        .post(&url)
        .bearer_auth(&cfg.service_key)
        .header("apikey", &cfg.service_key)
        .json(&serde_json::json!({ "expiresIn": VIEW_URL_TTL_SECS }))
        .send()
        .await
        .map_err(|e| {
            tracing::error!("could not sign url for {}: {}", key, e);
            AppError::InternalError("Failed to create file link".to_string())
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        tracing::error!("signing rejected for {}: {} {}", key, status, detail);
        return Err(AppError::NotFound("File not found".to_string()));
    }

    let signed = response.json::<SignedUrlResponse>().await.map_err(|e| {
        tracing::error!("could not parse signed url for {}: {}", key, e);
        AppError::InternalError("Failed to create file link".to_string())
    })?;

    // the api returns a relative path like /object/sign/bucket/key?token=...
    Ok(format!(
        "{}/storage/v1/{}",
        cfg.base,
        signed.signed_url.trim_start_matches('/')
    ))
}

// removes an object; a missing object is not treated as an error
pub async fn delete(key: &str) -> Result<(), AppError> {
    let cfg = config()?;
    let url = format!("{}/storage/v1/object/{}/{}", cfg.base, cfg.bucket, key);

    let response = http::storage()
        .delete(&url)
        .bearer_auth(&cfg.service_key)
        .header("apikey", &cfg.service_key)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("storage delete failed for {}: {}", key, e);
            AppError::InternalError("Failed to delete file".to_string())
        })?;

    if !response.status().is_success() && response.status().as_u16() != 404 {
        let status = response.status();
        let detail = response.text().await.unwrap_or_default();
        tracing::error!("storage delete rejected for {}: {} {}", key, status, detail);
        return Err(AppError::InternalError("Failed to delete file".to_string()));
    }

    tracing::info!("deleted {}", key);
    Ok(())
}

// best-effort delete used during cleanup — logs and moves on rather than
// failing an approval just because the image was already gone
pub async fn delete_quietly(key: &str) {
    if let Err(e) = delete(key).await {
        tracing::warn!("could not delete {}: {:?}", key, e);
    }
}
