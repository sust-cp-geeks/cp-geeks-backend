use crate::app_state::AppState;
use crate::services::storage;
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::time::Duration;

// how long we wait on the db before calling it unhealthy
const DB_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

// deliberately shorter than the db. storage is not needed to serve the api —
// only a manual signup touches it — and the storage client's own timeout is
// 30s, which would hang the health endpoint long past any monitor's patience.
const STORAGE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

// reported, never fatal. supabase pausing should show up here rather than
// first appearing as a failed signup, but taking the whole instance out of a
// load balancer over it would be a worse outcome than the thing it reports.
async fn storage_status() -> &'static str {
    if !storage::is_configured() {
        return "not_configured";
    }

    match tokio::time::timeout(STORAGE_PROBE_TIMEOUT, storage::probe()).await {
        Ok(Ok(())) => "ok",
        Ok(Err(e)) => {
            tracing::warn!("storage health check failed: {}", e);
            "unavailable"
        }
        Err(_) => {
            tracing::warn!(
                "storage health check timed out after {:?}",
                STORAGE_PROBE_TIMEOUT
            );
            "timeout"
        }
    }
}

// health check which verifies server + db are alive
// the status code has to reflect the result — a 200 with an error body reads as
// healthy to load balancers and uptime monitors, which is worse than no check
pub async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let probe = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.pool);

    match tokio::time::timeout(DB_PROBE_TIMEOUT, probe).await {
        // storage is only probed once the db is known good — a 503 should come
        // back fast rather than waiting on a dependency we are about to ignore
        Ok(Ok(_)) => {
            let storage = storage_status().await;
            (
                StatusCode::OK,
                Json(json!({
                    "status": "ok",
                    "database": "connected",
                    "storage": storage
                })),
            )
        }
        Ok(Err(e)) => {
            tracing::error!("db health check failed: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "error",
                    "database": "disconnected"
                })),
            )
        }
        // a hung db would otherwise hold the request open until the client gives up
        Err(_) => {
            tracing::error!("db health check timed out after {:?}", DB_PROBE_TIMEOUT);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "error",
                    "database": "timeout"
                })),
            )
        }
    }
}
