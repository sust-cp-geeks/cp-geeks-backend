use crate::app_state::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::time::Duration;

// how long we wait on the db before calling it unhealthy
const DB_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

// health check which verifies server + db are alive
// the status code has to reflect the result — a 200 with an error body reads as
// healthy to load balancers and uptime monitors, which is worse than no check
pub async fn health_check(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    let probe = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&state.pool);

    match tokio::time::timeout(DB_PROBE_TIMEOUT, probe).await {
        Ok(Ok(_)) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "database": "connected"
            })),
        ),
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
